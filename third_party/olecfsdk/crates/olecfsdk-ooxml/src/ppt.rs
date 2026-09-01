use std::collections::BTreeSet;

use olecfsdk::{
  office_art::{OfficeArtImageFormat, OfficeArtShapeFlags},
  ppt::{
    ColorSchemeAtom, PptFile, PptLiveImageLink, PptLiveImageSource, PptLiveImageStore,
    PptLiveMasterLink, PptLiveNotesLink, PptLivePersistObject, PptLivePersistObjectRole,
    PptLiveShapeRef, PptLiveSlideTransitionRef, PptLiveTableRef, PptLiveTextAtomRef,
    PptLiveTextBodyRef, PptPlaceholderSize, PptPlaceholderType, SlideShowSlideInfoAtom,
  },
};
use ooxmlsdk::{
  common::XmlNamespace,
  namespaces::XmlKnownNamespace,
  parts::{
    image_part::ImagePart, notes_master_part::NotesMasterPart, notes_slide_part::NotesSlidePart,
    presentation_document::PresentationDocument, presentation_part::PresentationPart,
    slide_layout_part::SlideLayoutPart, slide_master_part::SlideMasterPart, slide_part::SlidePart,
    theme_part::ThemePart,
  },
  schemas::{
    schemas_openxmlformats_org_drawingml_2006_main as a,
    schemas_openxmlformats_org_presentationml_2006_main::{
      self as p, CommonSlideData, NonVisualDrawingProperties, NonVisualShapeProperties,
      NotesMaster, NotesMasterId, NotesMasterIdList, NotesSize, NotesSlide, Presentation, Shape,
      ShapeProperties, ShapeTree, ShapeTreeChoice, Slide, SlideId, SlideIdList, SlideLayout,
      SlideLayoutId, SlideLayoutIdList, SlideMaster, SlideMasterId, SlideMasterIdList, SlideSize,
      TextBody,
    },
  },
  sdk::{PresentationDocumentType, SdkPart, SdkPartDescriptor},
  simple_type::{BooleanValue, CoordinateValue},
};

use crate::{
  ConversionCode, ConversionOptions, ConversionOutput, ConversionReport, Disposition, Error,
  LossPolicy, Result, SourceLocation, metadata::convert_core_properties,
};

/// Converts a typed PowerPoint 97–2003 root into a PresentationML package.
///
/// The default policy rejects the first known semantic loss. Use
/// [`convert_ppt_with_options`] to request an explicit diagnostic report.
pub fn convert_ppt(source: &PptFile) -> Result<ConversionOutput<PresentationDocument>> {
  convert_ppt_with_options(source, ConversionOptions::default())
}

/// Converts a typed PowerPoint 97–2003 root with an explicit loss policy.
pub fn convert_ppt_with_options(
  source: &PptFile,
  options: ConversionOptions,
) -> Result<ConversionOutput<PresentationDocument>> {
  let live = source.live_presentation()?;
  let image_store = source.live_image_store()?;
  let mut media = PptMediaState {
    store: &image_store,
    parts: Vec::new(),
  };
  let mut report = ConversionReport::default();
  let mut next_theme_index = 1usize;
  if live.handout_master_slide.is_some()
    || !live.active_x_controls.is_empty()
    || !live.embedded_ole_objects.is_empty()
    || !live.linked_ole_objects.is_empty()
    || live.vba_project.is_some()
  {
    unsupported(
      &mut report,
      options,
      ConversionCode::PresentationFeatureNotMapped,
      SourceLocation::PptPresentation,
    )?;
  }

  let slides = live.slides()?;
  let mut document = PresentationDocument::create(PresentationDocumentType::Presentation);
  let presentation_part = document.add_new_part_auto_id::<PresentationPart>()?;
  let slide_width = master_to_emu(live.document_atom.slide_size.x)?;
  let slide_height = master_to_emu(live.document_atom.slide_size.y)?;
  let slide_size = (slide_width, slide_height);
  let notes_width = master_to_emu(live.document_atom.notes_size.x)?;
  let notes_height = master_to_emu(live.document_atom.notes_size.y)?;
  let notes_size = (notes_width, notes_height);
  let notes_master = if let Some(source_master) = live.notes_master_slide {
    let part = presentation_part.add_new_part_auto_id::<_, NotesMasterPart>(&mut document)?;
    let color_scheme = source_master.color_scheme()?;
    add_ppt_theme(
      color_scheme.map(|scheme| scheme.value),
      &part,
      &mut document,
      &mut next_theme_index,
    )?;
    if color_scheme.is_some() {
      report.record(Disposition::Mapped);
    }
    let shape_tree = convert_shape_tree(
      source_master.shapes()?,
      PptShapeConversion {
        owner: PptShapeOwner::NotesMaster,
        slide_size: notes_size,
        host: &part,
        document: &mut document,
        media: &mut media,
        options,
        report: &mut report,
      },
    )?;
    part.set_root_element(
      &mut document,
      NotesMaster {
        xmlns: presentation_namespaces(),
        common_slide_data: Box::new(CommonSlideData {
          shape_tree: Box::new(shape_tree),
          ..Default::default()
        }),
        color_map: Box::new(default_color_map()),
        ..Default::default()
      },
    )?;
    unsupported(
      &mut report,
      options,
      ConversionCode::MasterFeatureNotMapped,
      SourceLocation::PptNotesMaster {
        persist_id: source_master.reference.persist_id,
      },
    )?;
    report.record(Disposition::Mapped);
    Some(part)
  } else {
    None
  };
  let mut masters = Vec::new();
  for (master_index, source_master) in live.master_slides.iter().copied().enumerate() {
    if source_master.role != PptLivePersistObjectRole::MainMasterSlide {
      continue;
    }
    let source_id = source_master_id(source_master, master_index)?;
    let part = presentation_part.add_new_part_auto_id::<_, SlideMasterPart>(&mut document)?;
    let color_scheme = source_master.color_scheme()?;
    add_ppt_theme(
      color_scheme.map(|scheme| scheme.value),
      &part,
      &mut document,
      &mut next_theme_index,
    )?;
    if color_scheme.is_some() {
      report.record(Disposition::Mapped);
    }
    let shape_tree = convert_shape_tree(
      source_master.shapes()?,
      PptShapeConversion {
        owner: PptShapeOwner::Master { master_index },
        slide_size,
        host: &part,
        document: &mut document,
        media: &mut media,
        options,
        report: &mut report,
      },
    )?;
    unsupported(
      &mut report,
      options,
      ConversionCode::MasterFeatureNotMapped,
      SourceLocation::PptMaster {
        master_index,
        slide_id: source_id,
      },
    )?;
    masters.push(ConvertedMaster {
      source_id,
      part,
      common_slide_data: Box::new(CommonSlideData {
        shape_tree: Box::new(shape_tree),
        ..Default::default()
      }),
      layouts: Vec::new(),
    });
  }

  for (master_index, source_master) in live.master_slides.iter().copied().enumerate() {
    if source_master.role != PptLivePersistObjectRole::TitleMasterSlide {
      continue;
    }
    let source_id = source_master_id(source_master, master_index)?;
    let (_, atom) = source_master.slide_atom()?.ok_or_else(|| {
      olecfsdk::Error::invalid(
        source_master.record().offset,
        "PPT title master has no SlideAtom",
      )
    })?;
    let Some(parent_index) = masters
      .iter()
      .position(|master| master.source_id == atom.master_id_ref)
    else {
      unsupported(
        &mut report,
        options,
        ConversionCode::MasterRelationshipNotMapped,
        SourceLocation::PptMaster {
          master_index,
          slide_id: source_id,
        },
      )?;
      continue;
    };
    let location = SourceLocation::PptMaster {
      master_index,
      slide_id: source_id,
    };
    add_layout(
      &mut document,
      &mut masters[parent_index],
      LayoutSource {
        source_master_id: Some(source_id),
        geometry: atom.geometry,
        content: LayoutContent::Shapes {
          shapes: source_master.shapes()?,
          owner: PptShapeOwner::Master { master_index },
          slide_size,
        },
        location,
      },
      &mut media,
      options,
      &mut report,
    )?;
    unsupported(
      &mut report,
      options,
      ConversionCode::MasterFeatureNotMapped,
      location,
    )?;
  }

  let mut slide_ids = Vec::with_capacity(slides.len());
  for (slide_index, source_slide) in slides.into_iter().enumerate() {
    let slide_location = SourceLocation::PptSlide {
      slide_index,
      slide_id: source_slide.id().value(),
    };
    let master_id = match source_slide.master {
      PptLiveMasterLink::Resolved(master) => source_master_id(
        *master,
        live
          .master_slides
          .iter()
          .position(|candidate| std::ptr::eq(candidate, master))
          .expect("resolved PPT master belongs to the live master list"),
      )?,
      PptLiveMasterLink::NotSpecified
      | PptLiveMasterLink::Missing { .. }
      | PptLiveMasterLink::Ambiguous { .. } => {
        unsupported(
          &mut report,
          options,
          ConversionCode::MasterRelationshipNotMapped,
          slide_location,
        )?;
        0
      }
    };
    let layout_part = if let Some((master_index, layout_index)) = find_layout(&masters, master_id) {
      Some(masters[master_index].layouts[layout_index].part.clone())
    } else if let Some(master_index) = masters
      .iter()
      .position(|master| master.source_id == master_id)
    {
      let layout_index = add_layout(
        &mut document,
        &mut masters[master_index],
        LayoutSource {
          source_master_id: None,
          geometry: source_slide.slide_atom.geometry,
          content: LayoutContent::Empty,
          location: slide_location,
        },
        &mut media,
        options,
        &mut report,
      )?;
      Some(masters[master_index].layouts[layout_index].part.clone())
    } else {
      unsupported(
        &mut report,
        options,
        ConversionCode::MasterRelationshipNotMapped,
        slide_location,
      )?;
      None
    };
    let slide_part = presentation_part.add_new_part_auto_id::<_, SlidePart>(&mut document)?;
    if let Some(layout_part) = layout_part {
      slide_part.add_part(&mut document, layout_part)?;
    }
    let shape_tree = convert_shape_tree(
      source_slide.shapes()?,
      PptShapeConversion {
        owner: PptShapeOwner::Slide { slide_index },
        slide_size,
        host: &slide_part,
        document: &mut document,
        media: &mut media,
        options,
        report: &mut report,
      },
    )?;
    let (transition, show) = convert_slide_transition(
      source_slide.transition()?,
      slide_location,
      options,
      &mut report,
    )?;
    slide_part.set_root_element(
      &mut document,
      Slide {
        xmlns: presentation_namespaces(),
        show,
        common_slide_data: Box::new(CommonSlideData {
          shape_tree: Box::new(shape_tree),
          ..Default::default()
        }),
        transition,
        ..Default::default()
      },
    )?;
    match source_slide.notes {
      PptLiveNotesLink::Resolved { object, .. } => {
        let notes_part = slide_part.add_new_part_auto_id::<_, NotesSlidePart>(&mut document)?;
        notes_part.add_part(&mut document, slide_part.clone())?;
        if let Some(notes_master) = &notes_master {
          notes_part.add_part(&mut document, notes_master.clone())?;
        } else {
          unsupported(
            &mut report,
            options,
            ConversionCode::MasterRelationshipNotMapped,
            SourceLocation::PptNotesSlide {
              slide_index,
              notes_id: object.reference.persist_id,
            },
          )?;
        }
        let shape_tree = convert_shape_tree(
          object.shapes()?,
          PptShapeConversion {
            owner: PptShapeOwner::NotesSlide { slide_index },
            slide_size: notes_size,
            host: &notes_part,
            document: &mut document,
            media: &mut media,
            options,
            report: &mut report,
          },
        )?;
        notes_part.set_root_element(
          &mut document,
          NotesSlide {
            xmlns: presentation_namespaces(),
            common_slide_data: Box::new(CommonSlideData {
              shape_tree: Box::new(shape_tree),
              ..Default::default()
            }),
            color_map_override: Some(Box::new(p::ColorMapOverride {
              color_map_override_choice: Some(p::ColorMapOverrideChoice::MasterColorMapping),
            })),
            ..Default::default()
          },
        )?;
        report.record(Disposition::Mapped);
      }
      PptLiveNotesLink::NotSpecified => report.record(Disposition::NotApplicable),
      PptLiveNotesLink::Missing { notes_id }
      | PptLiveNotesLink::Ambiguous { notes_id }
      | PptLiveNotesLink::SlideMismatch { notes_id, .. } => unsupported(
        &mut report,
        options,
        ConversionCode::MasterRelationshipNotMapped,
        SourceLocation::PptNotesSlide {
          slide_index,
          notes_id,
        },
      )?,
    }
    let relationship_id = presentation_part
      .get_id_of_part(&document, &slide_part)
      .expect("a newly added slide has a relationship id")
      .to_owned();
    let source_id = source_slide.id().value();
    let id = if (256..2_147_483_648).contains(&source_id) {
      source_id
    } else {
      u32::try_from(slide_index)
        .ok()
        .and_then(|value| value.checked_add(256))
        .ok_or_else(|| olecfsdk::Error::Limit("PPT slide index exceeds u32".into()))?
    };
    slide_ids.push(SlideId {
      id,
      relationship_id,
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }

  let mut master_ids = Vec::with_capacity(masters.len());
  for (target_index, master) in masters.into_iter().enumerate() {
    let layout_ids = master
      .layouts
      .iter()
      .enumerate()
      .map(|(layout_index, layout)| {
        let relationship_id = master
          .part
          .get_id_of_part(&document, &layout.part)
          .expect("a converted layout belongs to its slide master")
          .to_owned();
        Ok(SlideLayoutId {
          id: Some(large_list_id(layout_index)?),
          relationship_id,
          ..Default::default()
        })
      })
      .collect::<Result<Vec<_>>>()?;
    master.part.set_root_element(
      &mut document,
      SlideMaster {
        xmlns: presentation_namespaces(),
        common_slide_data: master.common_slide_data,
        color_map: Box::new(default_color_map()),
        slide_layout_id_list: Some(SlideLayoutIdList {
          slide_layout_id: layout_ids,
        }),
        ..Default::default()
      },
    )?;
    let relationship_id = presentation_part
      .get_id_of_part(&document, &master.part)
      .expect("a converted master belongs to the presentation")
      .to_owned();
    master_ids.push(SlideMasterId {
      id: Some(large_list_id(target_index)?),
      relationship_id,
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  let notes_master_id_list = notes_master.as_ref().map(|part| {
    let relationship_id = presentation_part
      .get_id_of_part(&document, part)
      .expect("a converted notes master belongs to the presentation")
      .to_owned();
    Box::new(NotesMasterIdList {
      notes_master_id: Some(Box::new(NotesMasterId {
        id: relationship_id,
        ..Default::default()
      })),
    })
  });
  presentation_part.set_root_element(
    &mut document,
    Presentation {
      xmlns: presentation_namespaces(),
      slide_master_id_list: Some(SlideMasterIdList {
        slide_master_id: master_ids,
      }),
      notes_master_id_list,
      slide_id_list: Some(SlideIdList {
        slide_id: slide_ids,
      }),
      slide_size: Some(SlideSize {
        cx: i32::try_from(slide_width)
          .map_err(|_| olecfsdk::Error::Limit("PPT slide width exceeds OOXML i32".into()))?,
        cy: i32::try_from(slide_height)
          .map_err(|_| olecfsdk::Error::Limit("PPT slide height exceeds OOXML i32".into()))?,
        ..Default::default()
      }),
      notes_size: NotesSize {
        cx: notes_width,
        cy: notes_height,
      },
      ..Default::default()
    },
  )?;
  report.record(Disposition::Mapped);
  if let Some(properties) = convert_core_properties(&source.shared, options, &mut report)? {
    let properties_part = document.add_core_file_properties_part()?;
    properties_part.set_root_element(&mut document, properties)?;
  }
  Ok(ConversionOutput { document, report })
}

fn convert_slide_transition(
  source: Option<PptLiveSlideTransitionRef<'_>>,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<(Option<Box<p::Transition>>, Option<BooleanValue>)> {
  let Some(source) = source else {
    return Ok((None, None));
  };
  let value = source.value;
  let mut transition_loss = false;
  let transition_choice = ppt_transition_choice(
    value.effect_type,
    value.effect_direction,
    &mut transition_loss,
  );
  let speed = match value.speed {
    0 => Some(p::TransitionSpeedValues::Slow),
    1 => Some(p::TransitionSpeedValues::Medium),
    2 => Some(p::TransitionSpeedValues::Fast),
    _ => {
      transition_loss = true;
      None
    }
  };
  if transition_loss {
    unsupported(
      report,
      options,
      ConversionCode::SlideTransitionNotMapped,
      location,
    )?;
  }

  const MANUAL_ADVANCE: u16 = 1 << 0;
  const HIDDEN: u16 = 1 << 2;
  const AUTO_ADVANCE: u16 = 1 << 10;
  let has_unmapped_features = ppt_transition_has_unmapped_features(value);
  if has_unmapped_features {
    unsupported(
      report,
      options,
      ConversionCode::SlideTransitionFeatureNotMapped,
      location,
    )?;
  }
  let advance_after_time = (value.flags & AUTO_ADVANCE != 0
    && (0..=86_399_000).contains(&value.slide_time))
  .then(|| value.slide_time.to_string());
  let show = (value.flags & HIDDEN != 0).then_some(false.into());
  report.record(Disposition::Mapped);
  Ok((
    Some(Box::new(p::Transition {
      speed,
      advance_on_click: Some((value.flags & MANUAL_ADVANCE != 0).into()),
      advance_after_time,
      transition_choice,
      ..Default::default()
    })),
    show,
  ))
}

fn ppt_transition_has_unmapped_features(value: &SlideShowSlideInfoAtom) -> bool {
  const SOUND: u16 = 1 << 4;
  const LOOP_SOUND: u16 = 1 << 6;
  const STOP_SOUND: u16 = 1 << 8;
  const AUTO_ADVANCE: u16 = 1 << 10;
  const CURSOR_VISIBLE: u16 = 1 << 12;
  value.flags & (SOUND | LOOP_SOUND | STOP_SOUND | CURSOR_VISIBLE) != 0
    || value.flags & AUTO_ADVANCE != 0 && !(0..=86_399_000).contains(&value.slide_time)
}

fn ppt_transition_choice(
  effect_type: u8,
  direction: u8,
  loss: &mut bool,
) -> Option<p::TransitionChoice> {
  use p::TransitionChoice as Choice;
  let choice = match effect_type {
    0 if direction == 0 => return None,
    0 if direction == 1 => Choice::CutTransition(p::CutTransition {
      through_black: Some(true.into()),
    }),
    // MS-PPT requires consumers to ignore effectDirection for Random.
    1 => Choice::RandomTransition,
    2 => Choice::BlindsTransition(p::BlindsTransition {
      direction: ppt_axis_direction(direction, loss),
    }),
    3 => Choice::CheckerTransition(p::CheckerTransition {
      direction: ppt_horizontal_first_direction(direction, loss),
    }),
    4 => Choice::CoverTransition(p::CoverTransition {
      direction: ppt_eight_way_direction(direction, loss),
    }),
    5 if direction == 0 => Choice::DissolveTransition,
    6 if direction == 0 => Choice::FadeTransition(p::FadeTransition::default()),
    7 => Choice::PullTransition(p::PullTransition {
      direction: ppt_eight_way_direction(direction, loss),
    }),
    8 => Choice::RandomBarTransition(p::RandomBarTransition {
      direction: ppt_horizontal_first_direction(direction, loss),
    }),
    9 => Choice::StripsTransition(p::StripsTransition {
      direction: ppt_corner_direction(direction, loss),
    }),
    10 => Choice::WipeTransition(p::WipeTransition {
      direction: ppt_slide_direction(direction, loss),
    }),
    11 => Choice::ZoomTransition(p::ZoomTransition {
      direction: ppt_in_out_direction(direction, loss),
    }),
    13 => {
      let (orientation, direction) = match direction {
        0 => (
          Some(p::DirectionValues::Horizontal),
          Some(p::TransitionInOutDirectionValues::Out),
        ),
        1 => (
          Some(p::DirectionValues::Horizontal),
          Some(p::TransitionInOutDirectionValues::In),
        ),
        2 => (
          Some(p::DirectionValues::Vertical),
          Some(p::TransitionInOutDirectionValues::Out),
        ),
        3 => (
          Some(p::DirectionValues::Vertical),
          Some(p::TransitionInOutDirectionValues::In),
        ),
        _ => {
          *loss = true;
          (None, None)
        }
      };
      Choice::SplitTransition(p::SplitTransition {
        orientation,
        direction,
      })
    }
    17 if direction == 0 => Choice::DiamondTransition,
    18 if direction == 0 => Choice::PlusTransition,
    19 if direction == 0 => Choice::WedgeTransition,
    20 => Choice::PushTransition(p::PushTransition {
      direction: ppt_slide_direction(direction, loss),
    }),
    21 => Choice::CombTransition(p::CombTransition {
      direction: ppt_horizontal_first_direction(direction, loss),
    }),
    22 if direction == 0 => Choice::NewsflashTransition,
    23 if direction == 0 => Choice::FadeTransition(p::FadeTransition::default()),
    26 if matches!(direction, 1 | 2 | 3 | 4 | 8) => Choice::WheelTransition(p::WheelTransition {
      spokes: Some(u32::from(direction)),
    }),
    27 if direction == 0 => Choice::CircleTransition,
    // MS-PPT defines 0xFF as undefined and requires consumers to ignore it.
    255 => return None,
    _ => {
      *loss = true;
      return None;
    }
  };
  Some(choice)
}

fn ppt_axis_direction(direction: u8, loss: &mut bool) -> Option<p::DirectionValues> {
  match direction {
    0 => Some(p::DirectionValues::Vertical),
    1 => Some(p::DirectionValues::Horizontal),
    _ => {
      *loss = true;
      None
    }
  }
}

fn ppt_horizontal_first_direction(direction: u8, loss: &mut bool) -> Option<p::DirectionValues> {
  match direction {
    0 => Some(p::DirectionValues::Horizontal),
    1 => Some(p::DirectionValues::Vertical),
    _ => {
      *loss = true;
      None
    }
  }
}

fn ppt_slide_direction(
  direction: u8,
  loss: &mut bool,
) -> Option<p::TransitionSlideDirectionValues> {
  match direction {
    0 => Some(p::TransitionSlideDirectionValues::Left),
    1 => Some(p::TransitionSlideDirectionValues::Up),
    2 => Some(p::TransitionSlideDirectionValues::Right),
    3 => Some(p::TransitionSlideDirectionValues::Down),
    _ => {
      *loss = true;
      None
    }
  }
}

fn ppt_eight_way_direction(direction: u8, loss: &mut bool) -> Option<String> {
  match direction {
    0 => Some("l".into()),
    1 => Some("u".into()),
    2 => Some("r".into()),
    3 => Some("d".into()),
    4 => Some("lu".into()),
    5 => Some("ru".into()),
    6 => Some("ld".into()),
    7 => Some("rd".into()),
    _ => {
      *loss = true;
      None
    }
  }
}

fn ppt_corner_direction(
  direction: u8,
  loss: &mut bool,
) -> Option<p::TransitionCornerDirectionValues> {
  match direction {
    4 => Some(p::TransitionCornerDirectionValues::LeftUp),
    5 => Some(p::TransitionCornerDirectionValues::RightUp),
    6 => Some(p::TransitionCornerDirectionValues::LeftDown),
    7 => Some(p::TransitionCornerDirectionValues::RightDown),
    _ => {
      *loss = true;
      None
    }
  }
}

fn ppt_in_out_direction(
  direction: u8,
  loss: &mut bool,
) -> Option<p::TransitionInOutDirectionValues> {
  match direction {
    0 => Some(p::TransitionInOutDirectionValues::Out),
    1 => Some(p::TransitionInOutDirectionValues::In),
    _ => {
      *loss = true;
      None
    }
  }
}

struct ConvertedMaster {
  source_id: u32,
  part: SlideMasterPart,
  common_slide_data: Box<CommonSlideData>,
  layouts: Vec<ConvertedLayout>,
}

struct ConvertedLayout {
  source_master_id: Option<u32>,
  geometry: u32,
  part: SlideLayoutPart,
}

struct PptMediaState<'a> {
  store: &'a PptLiveImageStore<'a>,
  parts: Vec<(PptLiveImageSource, ImagePart)>,
}

enum LayoutContent<'a> {
  Empty,
  Shapes {
    shapes: Vec<PptLiveShapeRef<'a>>,
    owner: PptShapeOwner,
    slide_size: (i64, i64),
  },
}

struct LayoutSource<'a> {
  source_master_id: Option<u32>,
  geometry: u32,
  content: LayoutContent<'a>,
  location: SourceLocation,
}

#[derive(Clone, Copy)]
enum PptShapeOwner {
  Master { master_index: usize },
  Slide { slide_index: usize },
  NotesMaster,
  NotesSlide { slide_index: usize },
}

struct PptShapeConversion<'source, 'state, P> {
  owner: PptShapeOwner,
  slide_size: (i64, i64),
  host: &'state P,
  document: &'state mut PresentationDocument,
  media: &'state mut PptMediaState<'source>,
  options: ConversionOptions,
  report: &'state mut ConversionReport,
}

impl PptShapeOwner {
  const fn location(self, shape_id: u32) -> SourceLocation {
    match self {
      Self::Master { master_index } => SourceLocation::PptMasterShape {
        master_index,
        shape_id,
      },
      Self::Slide { slide_index } => SourceLocation::PptShape {
        slide_index,
        shape_id,
      },
      Self::NotesMaster => SourceLocation::PptNotesMasterShape { shape_id },
      Self::NotesSlide { slide_index } => SourceLocation::PptNotesShape {
        slide_index,
        shape_id,
      },
    }
  }
}

fn source_master_id(source: PptLivePersistObject<'_>, master_index: usize) -> Result<u32> {
  source
    .slide_persist()
    .map(|persist| persist.slide_id)
    .ok_or_else(|| {
      olecfsdk::Error::invalid(
        source.source_record().offset,
        format!("PPT master {master_index} has no SlidePersistAtom identity"),
      )
      .into()
    })
}

fn find_layout(masters: &[ConvertedMaster], source_master_id: u32) -> Option<(usize, usize)> {
  masters
    .iter()
    .enumerate()
    .find_map(|(master_index, master)| {
      master
        .layouts
        .iter()
        .position(|layout| layout.source_master_id == Some(source_master_id))
        .map(|layout_index| (master_index, layout_index))
    })
}

fn add_layout<'source>(
  document: &mut PresentationDocument,
  master: &mut ConvertedMaster,
  source: LayoutSource<'source>,
  media: &mut PptMediaState<'source>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<usize> {
  if source.source_master_id.is_none()
    && let Some(index) = master
      .layouts
      .iter()
      .position(|layout| layout.source_master_id.is_none() && layout.geometry == source.geometry)
  {
    return Ok(index);
  }
  let (layout_type, exact) = slide_layout_type(source.geometry);
  if !exact {
    unsupported(
      report,
      options,
      ConversionCode::SlideFeatureNotMapped,
      source.location,
    )?;
  }
  let part = master
    .part
    .add_new_part_auto_id::<_, SlideLayoutPart>(document)?;
  part.add_part(document, master.part.clone())?;
  let common_slide_data = match source.content {
    LayoutContent::Empty => Box::new(empty_common_slide_data()),
    LayoutContent::Shapes {
      shapes,
      owner,
      slide_size,
    } => Box::new(CommonSlideData {
      shape_tree: Box::new(convert_shape_tree(
        shapes,
        PptShapeConversion {
          owner,
          slide_size,
          host: &part,
          document,
          media,
          options,
          report,
        },
      )?),
      ..Default::default()
    }),
  };
  part.set_root_element(
    document,
    SlideLayout {
      xmlns: presentation_namespaces(),
      matching_name: Some(format!("Legacy layout {}", source.geometry)),
      r#type: Some(layout_type),
      preserve: Some(BooleanValue::True),
      common_slide_data,
      color_map_override: Some(Box::new(p::ColorMapOverride {
        color_map_override_choice: Some(p::ColorMapOverrideChoice::MasterColorMapping),
      })),
      ..Default::default()
    },
  )?;
  master.layouts.push(ConvertedLayout {
    source_master_id: source.source_master_id,
    geometry: source.geometry,
    part,
  });
  report.record(Disposition::Mapped);
  Ok(master.layouts.len() - 1)
}

fn convert_shape_tree<'source, P: SdkPart>(
  source_shapes: Vec<PptLiveShapeRef<'source>>,
  conversion: PptShapeConversion<'source, '_, P>,
) -> Result<ShapeTree> {
  let PptShapeConversion {
    owner,
    slide_size,
    host,
    document,
    media,
    options,
    report,
  } = conversion;
  let mut shapes = Vec::with_capacity(source_shapes.len());
  let mut converted_tables = Vec::new();
  let mut table_child_records = BTreeSet::new();
  for source_shape in &source_shapes {
    if let Some(table) = source_shape.table()? {
      table_child_records.extend(
        table
          .cells
          .iter()
          .map(|cell| std::ptr::from_ref(cell.shape.source_record).addr()),
      );
      table_child_records.extend(
        table
          .borders
          .iter()
          .map(|border| std::ptr::from_ref(border.source_record).addr()),
      );
      converted_tables.push((std::ptr::from_ref(source_shape.source_record).addr(), table));
    }
  }
  let source_root_shape_id = source_shapes
    .iter()
    .rev()
    .find(|shape| {
      shape
        .shape
        .flags
        .contains(OfficeArtShapeFlags::GROUP | OfficeArtShapeFlags::PATRIARCH)
    })
    .map_or(0, |shape| shape.shape_id());
  let root_shape_id = source_root_shape_id.max(1);
  let mut used_shape_ids = BTreeSet::from([root_shape_id]);
  if source_root_shape_id == 0 {
    unsupported(
      report,
      options,
      ConversionCode::ShapeIdentityNotMapped,
      owner.location(0),
    )?;
  }
  for source_shape in source_shapes {
    let source_record_address = std::ptr::from_ref(source_shape.source_record).addr();
    if table_child_records.contains(&source_record_address) {
      continue;
    }
    if source_shape
      .shape
      .flags
      .contains(OfficeArtShapeFlags::GROUP | OfficeArtShapeFlags::PATRIARCH)
    {
      report.record(Disposition::Mapped);
      continue;
    }
    let source_shape_id = source_shape.shape_id();
    let (shape_id, exact_shape_id) = allocate_shape_id(source_shape_id, &mut used_shape_ids)?;
    if !exact_shape_id {
      unsupported(
        report,
        options,
        ConversionCode::ShapeIdentityNotMapped,
        owner.location(source_shape_id),
      )?;
    }
    if let Some((_, table)) = converted_tables
      .iter()
      .find(|(address, _)| *address == source_record_address)
    {
      shapes.push(ShapeTreeChoice::GraphicFrame(Box::new(
        convert_table_shape(table, shape_id, owner, options, report)?,
      )));
      continue;
    }
    if let Some(blip_identifier) = source_shape.primary_blip_identifier()? {
      let source_location = owner.location(source_shape_id);
      if let Some(relationship_id) =
        add_ppt_image_relationship(blip_identifier, host, document, media)?
      {
        let shape_properties = convert_shape_geometry(source_shape, slide_size)?;
        if shape_properties.is_none() {
          unsupported(
            report,
            options,
            ConversionCode::ShapeGeometryNotMapped,
            source_location,
          )?;
        }
        if source_shape.outline_text.is_some() || !source_shape.text_bodies().is_empty() {
          unsupported(
            report,
            options,
            ConversionCode::PictureTextNotMapped,
            source_location,
          )?;
        }
        shapes.push(ShapeTreeChoice::Picture(Box::new(p::Picture {
          non_visual_picture_properties: Box::new(p::NonVisualPictureProperties {
            non_visual_drawing_properties: Box::new(NonVisualDrawingProperties {
              id: shape_id,
              name: format!("Picture {shape_id}"),
              ..Default::default()
            }),
            non_visual_picture_drawing_properties: Box::default(),
            application_non_visual_drawing_properties: Box::new(
              p::ApplicationNonVisualDrawingProperties {
                placeholder_shape: source_shape
                  .placeholder
                  .and_then(convert_placeholder)
                  .map(Box::new),
                ..Default::default()
              },
            ),
          }),
          blip_fill: Some(Box::new(p::BlipFill {
            blip: Some(Box::new(a::Blip {
              embed: Some(relationship_id),
              ..Default::default()
            })),
            blip_fill_choice: Some(p::BlipFillChoice::Stretch(Box::default())),
            ..Default::default()
          })),
          shape_properties: Box::new(shape_properties.unwrap_or_default()),
          ..Default::default()
        })));
        report.record(Disposition::Mapped);
        continue;
      }
      unsupported(
        report,
        options,
        ConversionCode::PictureNotMapped,
        source_location,
      )?;
    }
    shapes.push(ShapeTreeChoice::Shape(Box::new(convert_shape(
      source_shape,
      shape_id,
      owner,
      slide_size,
      options,
      report,
    )?)));
  }
  Ok(ShapeTree {
    non_visual_group_shape_properties: Box::new(p::NonVisualGroupShapeProperties {
      non_visual_drawing_properties: Box::new(NonVisualDrawingProperties {
        id: root_shape_id,
        name: "Shape Tree".into(),
        ..Default::default()
      }),
      ..Default::default()
    }),
    shape_tree_choice: shapes,
    ..Default::default()
  })
}

fn convert_table_shape(
  source: &PptLiveTableRef<'_>,
  shape_id: u32,
  owner: PptShapeOwner,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<p::GraphicFrame> {
  let source_location = owner.location(source.shape.shape_id());
  unsupported(
    report,
    options,
    ConversionCode::TableFormattingNotMapped,
    source_location,
  )?;
  let column_count = source.columns.len();
  let mut cell_owners = vec![None; source.rows.len() * column_count];
  for cell in &source.cells {
    for row in cell.row..cell.row + cell.row_span {
      for column in cell.column..cell.column + cell.column_span {
        cell_owners[row * column_count + column] = Some(cell);
      }
    }
  }
  let table_row = source
    .rows
    .iter()
    .enumerate()
    .map(|(row_index, row)| {
      let table_cell = source
        .columns
        .iter()
        .enumerate()
        .map(|(column_index, _)| {
          let cell = cell_owners[row_index * column_count + column_index]
            .expect("validated PPT table grid has an owner for every slot");
          let is_origin = row_index == cell.row && column_index == cell.column;
          let text_body = if is_origin {
            let mut bodies = cell.shape.text_bodies();
            if let Some(outline) = cell.shape.outline_text {
              bodies.insert(0, outline.text_body);
            }
            if !bodies.is_empty() {
              unsupported(
                report,
                options,
                ConversionCode::TextFormattingNotMapped,
                owner.location(cell.shape.shape_id()),
              )?;
            }
            let mut paragraphs = Vec::new();
            for body in bodies {
              append_text_body(
                body,
                &mut paragraphs,
                owner.location(cell.shape.shape_id()),
                options,
                report,
              )?;
            }
            Some(Box::new(a::TextBody {
              body_properties: Box::default(),
              paragraph: paragraphs,
              ..Default::default()
            }))
          } else {
            None
          };
          if is_origin {
            report.record(Disposition::Mapped);
          }
          Ok(a::TableCell {
            row_span: (is_origin && cell.row_span > 1)
              .then(|| i32::try_from(cell.row_span))
              .transpose()
              .map_err(|_| olecfsdk::Error::Limit("PPT table row span exceeds i32".into()))?,
            grid_span: (is_origin && cell.column_span > 1)
              .then(|| i32::try_from(cell.column_span))
              .transpose()
              .map_err(|_| olecfsdk::Error::Limit("PPT table column span exceeds i32".into()))?,
            horizontal_merge: (!is_origin && column_index > cell.column)
              .then_some(BooleanValue::True),
            vertical_merge: (!is_origin && row_index > cell.row).then_some(BooleanValue::True),
            text_body,
            table_cell_properties: Some(Box::default()),
            ..Default::default()
          })
        })
        .collect::<Result<Vec<_>>>()?;
      Ok(a::TableRow {
        height: CoordinateValue::Emu(master_to_emu(row.end - row.start)?),
        table_cell,
        ..Default::default()
      })
    })
    .collect::<Result<Vec<_>>>()?;
  for border in &source.borders {
    unsupported(
      report,
      options,
      ConversionCode::TableFormattingNotMapped,
      owner.location(border.shape_id()),
    )?;
  }
  let width = source
    .anchor
    .right
    .checked_sub(source.anchor.left)
    .filter(|value| *value >= 0)
    .ok_or_else(|| {
      olecfsdk::Error::invalid(
        source.shape.source_record.offset,
        "PPT table outer width is reversed",
      )
    })?;
  let height = source
    .anchor
    .bottom
    .checked_sub(source.anchor.top)
    .filter(|value| *value >= 0)
    .ok_or_else(|| {
      olecfsdk::Error::invalid(
        source.shape.source_record.offset,
        "PPT table outer height is reversed",
      )
    })?;
  report.record(Disposition::Mapped);
  Ok(p::GraphicFrame {
    non_visual_graphic_frame_properties: Box::new(p::NonVisualGraphicFrameProperties {
      non_visual_drawing_properties: Box::new(NonVisualDrawingProperties {
        id: shape_id,
        name: format!("Table {shape_id}"),
        ..Default::default()
      }),
      ..Default::default()
    }),
    transform: Box::new(p::Transform {
      offset: Some(a::Offset {
        x: CoordinateValue::Emu(master_to_emu(source.anchor.left)?),
        y: CoordinateValue::Emu(master_to_emu(source.anchor.top)?),
      }),
      extents: Some(a::Extents {
        cx: CoordinateValue::Emu(master_to_emu(width)?),
        cy: CoordinateValue::Emu(master_to_emu(height)?),
      }),
      ..Default::default()
    }),
    graphic: Box::new(a::Graphic {
      xmlns: Vec::new(),
      graphic_data: a::GraphicData {
        uri: "http://schemas.openxmlformats.org/drawingml/2006/table".into(),
        graphic_data_choice: vec![a::GraphicDataChoice::Table(Box::new(a::Table {
          table_properties: Some(Box::default()),
          table_grid: a::TableGrid {
            grid_column: source
              .columns
              .iter()
              .map(|column| {
                Ok(a::GridColumn {
                  width: CoordinateValue::Emu(master_to_emu(column.end - column.start)?),
                  ..Default::default()
                })
              })
              .collect::<Result<Vec<_>>>()?,
          },
          table_row,
        }))],
      },
    }),
    ..Default::default()
  })
}

fn allocate_shape_id(source_id: u32, used: &mut BTreeSet<u32>) -> Result<(u32, bool)> {
  if source_id != 0 && used.insert(source_id) {
    return Ok((source_id, true));
  }
  let mut candidate = 1u32;
  while !used.insert(candidate) {
    candidate = candidate
      .checked_add(1)
      .ok_or_else(|| olecfsdk::Error::Limit("PPT shape ID space is exhausted".into()))?;
  }
  Ok((candidate, false))
}

fn add_ppt_image_relationship<P: SdkPart>(
  blip_identifier: u32,
  host: &P,
  document: &mut PresentationDocument,
  media: &mut PptMediaState<'_>,
) -> Result<Option<String>> {
  let Some(zero_based) = blip_identifier.checked_sub(1) else {
    return Ok(None);
  };
  let Some(link) = usize::try_from(zero_based)
    .ok()
    .and_then(|index| media.store.entries.get(index))
  else {
    return Ok(None);
  };
  let PptLiveImageLink::Resolved(source) = *link else {
    return Ok(None);
  };
  let Some(content_type) = ppt_image_content_type(source.image.format) else {
    return Ok(None);
  };
  let image_part = if let Some((_, part)) = media
    .parts
    .iter()
    .find(|(identity, _)| *identity == source.source)
  {
    host.add_part(document, part.clone())?
  } else {
    let part = host.add_image_part(document, content_type)?;
    part.set_data(document, source.image.data.to_vec())?;
    media.parts.push((source.source, part.clone()));
    part
  };
  Ok(
    host
      .get_id_of_part(document, &image_part)
      .map(str::to_owned),
  )
}

const fn ppt_image_content_type(format: OfficeArtImageFormat) -> Option<&'static str> {
  match format {
    OfficeArtImageFormat::Emf => Some("image/x-emf"),
    OfficeArtImageFormat::Wmf => Some("image/x-wmf"),
    OfficeArtImageFormat::Jpeg => Some("image/jpeg"),
    OfficeArtImageFormat::Png => Some("image/png"),
    OfficeArtImageFormat::Tiff => Some("image/tiff"),
    OfficeArtImageFormat::Pict | OfficeArtImageFormat::Dib => None,
  }
}

fn empty_common_slide_data() -> CommonSlideData {
  CommonSlideData {
    shape_tree: Box::new(ShapeTree {
      non_visual_group_shape_properties: Box::new(p::NonVisualGroupShapeProperties {
        non_visual_drawing_properties: Box::new(NonVisualDrawingProperties {
          id: 1,
          name: "Shape Tree".into(),
          ..Default::default()
        }),
        ..Default::default()
      }),
      ..Default::default()
    }),
    ..Default::default()
  }
}

const fn slide_layout_type(geometry: u32) -> (p::SlideLayoutValues, bool) {
  match geometry {
    0 | 2 => (p::SlideLayoutValues::Title, true),
    1 => (p::SlideLayoutValues::Text, true),
    7 => (p::SlideLayoutValues::TitleOnly, true),
    8 => (p::SlideLayoutValues::TwoColumnText, true),
    14 => (p::SlideLayoutValues::FourObjects, true),
    15 => (p::SlideLayoutValues::ObjectOnly, true),
    16 => (p::SlideLayoutValues::Blank, true),
    17 => (p::SlideLayoutValues::VerticalTitleAndText, true),
    _ => (p::SlideLayoutValues::Custom, false),
  }
}

fn default_color_map() -> p::ColorMap {
  p::ColorMap {
    background1: a::ColorSchemeIndexValues::Light1,
    text1: a::ColorSchemeIndexValues::Dark1,
    background2: a::ColorSchemeIndexValues::Light2,
    text2: a::ColorSchemeIndexValues::Dark2,
    accent1: a::ColorSchemeIndexValues::Accent1,
    accent2: a::ColorSchemeIndexValues::Accent2,
    accent3: a::ColorSchemeIndexValues::Accent3,
    accent4: a::ColorSchemeIndexValues::Accent4,
    accent5: a::ColorSchemeIndexValues::Accent5,
    accent6: a::ColorSchemeIndexValues::Accent6,
    hyperlink: a::ColorSchemeIndexValues::Hyperlink,
    followed_hyperlink: a::ColorSchemeIndexValues::FollowedHyperlink,
    ..Default::default()
  }
}

fn add_ppt_theme<P: SdkPart>(
  source: Option<&ColorSchemeAtom>,
  host: &P,
  document: &mut PresentationDocument,
  next_theme_index: &mut usize,
) -> Result<ThemePart> {
  let relationship_id = host.next_relationship_id(document)?;
  let path = format!("ppt/theme/theme{}.xml", *next_theme_index);
  *next_theme_index = next_theme_index
    .checked_add(1)
    .ok_or_else(|| olecfsdk::Error::Limit("PPT theme count exceeds usize".into()))?;
  let part = host.add_new_part_with_content_type_and_path::<_, ThemePart>(
    document,
    relationship_id,
    <ThemePart as SdkPartDescriptor>::CONTENT_TYPE,
    path,
  )?;
  part.set_root_element(document, legacy_ppt_theme(source))?;
  Ok(part)
}

fn legacy_ppt_theme(source: Option<&ColorSchemeAtom>) -> a::Theme {
  // These are the PowerPoint 97 default ColorSchemeAtom values also used by
  // Apache POI when a new legacy slide is created. A conforming master
  // normally supplies its own active palette; the fallback only makes a
  // structurally complete theme for malformed/minimal producers.
  const DEFAULT_COLORS: [u32; 8] = [
    0x00ff_ffff,
    0x0000_0000,
    0x0080_8080,
    0x0000_0000,
    0x0099_cc00,
    0x00cc_3333,
    0x00ff_cccc,
    0x00b2_b2b2,
  ];
  let colors = source.map_or(&DEFAULT_COLORS, |scheme| &scheme.colors);
  let &[
    background,
    text_and_lines,
    shadow,
    title_text,
    fills,
    accent,
    hyperlink,
    followed,
  ] = colors;
  let color_scheme = a::ColorScheme {
    name: "Legacy PPT palette".into(),
    dark1_color: Box::new(a::Dark1Color {
      dark1_color_choice: Some(a::Dark1ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        text_and_lines,
      ))),
    }),
    light1_color: Box::new(a::Light1Color {
      light1_color_choice: Some(a::Light1ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        background,
      ))),
    }),
    dark2_color: Box::new(a::Dark2Color {
      dark2_color_choice: Some(a::Dark2ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        title_text,
      ))),
    }),
    light2_color: Box::new(a::Light2Color {
      light2_color_choice: Some(a::Light2ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        shadow,
      ))),
    }),
    accent1_color: Box::new(a::Accent1Color {
      accent1_color_choice: Some(a::Accent1ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        fills,
      ))),
    }),
    accent2_color: Box::new(a::Accent2Color {
      accent2_color_choice: Some(a::Accent2ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        accent,
      ))),
    }),
    accent3_color: Box::new(a::Accent3Color {
      accent3_color_choice: Some(a::Accent3ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        hyperlink,
      ))),
    }),
    accent4_color: Box::new(a::Accent4Color {
      accent4_color_choice: Some(a::Accent4ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        followed,
      ))),
    }),
    // The legacy palette has only two general accent slots. Reusing them
    // retains the source colors without manufacturing extra document data.
    accent5_color: Box::new(a::Accent5Color {
      accent5_color_choice: Some(a::Accent5ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        fills,
      ))),
    }),
    accent6_color: Box::new(a::Accent6Color {
      accent6_color_choice: Some(a::Accent6ColorChoice::RgbColorModelHex(ppt_theme_rgb(
        accent,
      ))),
    }),
    hyperlink: Box::new(a::Hyperlink {
      hyperlink_choice: Some(a::HyperlinkChoice::RgbColorModelHex(ppt_theme_rgb(
        hyperlink,
      ))),
    }),
    followed_hyperlink_color: Box::new(a::FollowedHyperlinkColor {
      followed_hyperlink_color_choice: Some(a::FollowedHyperlinkColorChoice::RgbColorModelHex(
        ppt_theme_rgb(followed),
      )),
    }),
    ..Default::default()
  };
  a::Theme {
    xmlns: vec![XmlNamespace::known(XmlKnownNamespace::A)],
    name: Some("Legacy PPT theme".into()),
    theme_elements: Box::new(a::ThemeElements {
      color_scheme: Box::new(color_scheme),
      font_scheme: Box::new(a::FontScheme {
        name: "Legacy PPT fonts".into(),
        major_font: Box::new(ppt_theme_major_font()),
        minor_font: Box::new(ppt_theme_minor_font()),
        ..Default::default()
      }),
      format_scheme: Box::new(ppt_theme_format_scheme(colors)),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn ppt_theme_rgb(value: u32) -> Box<a::RgbColorModelHex> {
  let bytes = value.to_le_bytes();
  Box::new(a::RgbColorModelHex {
    val: format!("{:02X}{:02X}{:02X}", bytes[0], bytes[1], bytes[2]),
    ..Default::default()
  })
}

fn ppt_theme_major_font() -> a::MajorFont {
  a::MajorFont {
    latin_font: Box::new(a::LatinFont {
      typeface: Some("Arial".into()),
      ..Default::default()
    }),
    east_asian_font: Box::new(a::EastAsianFont {
      typeface: Some(String::new()),
      ..Default::default()
    }),
    complex_script_font: Box::new(a::ComplexScriptFont {
      typeface: Some(String::new()),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn ppt_theme_minor_font() -> a::MinorFont {
  a::MinorFont {
    latin_font: Box::new(a::LatinFont {
      typeface: Some("Arial".into()),
      ..Default::default()
    }),
    east_asian_font: Box::new(a::EastAsianFont {
      typeface: Some(String::new()),
      ..Default::default()
    }),
    complex_script_font: Box::new(a::ComplexScriptFont {
      typeface: Some(String::new()),
      ..Default::default()
    }),
    ..Default::default()
  }
}

fn ppt_theme_format_scheme(colors: &[u32; 8]) -> a::FormatScheme {
  a::FormatScheme {
    name: Some("Legacy PPT formatting".into()),
    fill_style_list: a::FillStyleList {
      fill_style_list_choice: [colors[4], colors[5], colors[2]]
        .into_iter()
        .map(|color| a::FillStyleListChoice::SolidFill(Box::new(ppt_theme_solid_fill(color))))
        .collect(),
    },
    line_style_list: a::LineStyleList {
      outline: [9525, 25_400, 38_100]
        .into_iter()
        .map(|width| a::Outline {
          width: Some(width),
          outline_choice1: Some(a::OutlineChoice::SolidFill(Box::new(ppt_theme_solid_fill(
            colors[1],
          )))),
          ..Default::default()
        })
        .collect(),
    },
    effect_style_list: a::EffectStyleList {
      effect_style: (0..3)
        .map(|_| a::EffectStyle {
          effect_style_choice: Some(a::EffectStyleChoice::EffectList(Box::default())),
          ..Default::default()
        })
        .collect(),
    },
    background_fill_style_list: a::BackgroundFillStyleList {
      background_fill_style_list_choice: [colors[0], colors[4], colors[5]]
        .into_iter()
        .map(|color| {
          a::BackgroundFillStyleListChoice::SolidFill(Box::new(ppt_theme_solid_fill(color)))
        })
        .collect(),
    },
  }
}

fn ppt_theme_solid_fill(color: u32) -> a::SolidFill {
  a::SolidFill {
    solid_fill_choice: Some(a::SolidFillChoice::RgbColorModelHex(ppt_theme_rgb(color))),
    ..Default::default()
  }
}

fn presentation_namespaces() -> Vec<XmlNamespace> {
  vec![
    XmlNamespace::known(XmlKnownNamespace::P),
    XmlNamespace::known(XmlKnownNamespace::A),
    XmlNamespace::known(XmlKnownNamespace::R),
  ]
}

fn large_list_id(index: usize) -> Result<u32> {
  u32::try_from(index)
    .ok()
    .and_then(|value| value.checked_add(2_147_483_648))
    .ok_or_else(|| olecfsdk::Error::Limit("PPT master/layout count exceeds u32".into()).into())
}

fn convert_shape(
  source: PptLiveShapeRef<'_>,
  shape_id: u32,
  owner: PptShapeOwner,
  slide_size: (i64, i64),
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Shape> {
  let source_location = owner.location(source.shape_id());
  let shape_properties = convert_shape_geometry(source, slide_size)?;
  if shape_properties.is_none() {
    unsupported(
      report,
      options,
      ConversionCode::ShapeGeometryNotMapped,
      source_location,
    )?;
  }
  let mut text_bodies = source.text_bodies();
  if let Some(outline) = source.outline_text {
    text_bodies.insert(0, outline.text_body);
  }
  if text_bodies.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::MultipleTextBodiesNotMapped,
      source_location,
    )?;
  }
  let text_body = if text_bodies.is_empty() {
    None
  } else {
    unsupported(
      report,
      options,
      ConversionCode::TextFormattingNotMapped,
      source_location,
    )?;
    let mut paragraphs = Vec::new();
    for body in text_bodies {
      append_text_body(body, &mut paragraphs, source_location, options, report)?;
    }
    Some(Box::new(TextBody {
      body_properties: Box::default(),
      paragraph: paragraphs,
      ..Default::default()
    }))
  };
  report.record(Disposition::Mapped);
  Ok(Shape {
    non_visual_shape_properties: Box::new(NonVisualShapeProperties {
      non_visual_drawing_properties: Box::new(NonVisualDrawingProperties {
        id: shape_id,
        name: format!("Shape {shape_id}"),
        ..Default::default()
      }),
      application_non_visual_drawing_properties: Box::new(
        p::ApplicationNonVisualDrawingProperties {
          placeholder_shape: source
            .placeholder
            .and_then(convert_placeholder)
            .map(Box::new),
          ..Default::default()
        },
      ),
      ..Default::default()
    }),
    shape_properties: Box::new(shape_properties.unwrap_or_default()),
    text_body,
    ..Default::default()
  })
}

fn convert_shape_geometry(
  source: PptLiveShapeRef<'_>,
  slide_size: (i64, i64),
) -> Result<Option<ShapeProperties>> {
  if source.is_nested_group_child() {
    return Ok(None);
  }
  let preset = match native_shape_type(source.shape_type()) {
    Some(value) => value,
    None => return Ok(None),
  };
  let (x, y, cx, cy) = if source.shape.flags.contains(OfficeArtShapeFlags::BACKGROUND) {
    (0, 0, slide_size.0, slide_size.1)
  } else {
    let Some(anchor) = source.anchor()? else {
      return Ok(None);
    };
    let width = anchor.right.checked_sub(anchor.left);
    let height = anchor.bottom.checked_sub(anchor.top);
    let (Some(width), Some(height)) = (width, height) else {
      return Ok(None);
    };
    if width < 0
      || height < 0
      || [anchor.left, anchor.top, anchor.right, anchor.bottom].contains(&-1)
    {
      return Ok(None);
    }
    (
      master_to_emu(anchor.left)?,
      master_to_emu(anchor.top)?,
      master_to_emu(width)?,
      master_to_emu(height)?,
    )
  };
  Ok(Some(ShapeProperties {
    transform2_d: Some(Box::new(a::Transform2D {
      horizontal_flip: source
        .shape
        .flags
        .contains(OfficeArtShapeFlags::FLIP_HORIZONTAL)
        .then_some(BooleanValue::True),
      vertical_flip: source
        .shape
        .flags
        .contains(OfficeArtShapeFlags::FLIP_VERTICAL)
        .then_some(BooleanValue::True),
      offset: Some(a::Offset {
        x: CoordinateValue::Emu(x),
        y: CoordinateValue::Emu(y),
      }),
      extents: Some(a::Extents {
        cx: CoordinateValue::Emu(cx),
        cy: CoordinateValue::Emu(cy),
      }),
      ..Default::default()
    })),
    shape_properties_choice1: Some(p::ShapePropertiesChoice::PresetGeometry(Box::new(
      a::PresetGeometry {
        preset,
        ..Default::default()
      },
    ))),
    ..Default::default()
  }))
}

fn convert_placeholder(source: &olecfsdk::ppt::PlaceholderAtom) -> Option<p::PlaceholderShape> {
  let (r#type, orientation) = match source.placement_id {
    PptPlaceholderType::None => return None,
    PptPlaceholderType::MasterTitle | PptPlaceholderType::Title => {
      (p::PlaceholderValues::Title, None)
    }
    PptPlaceholderType::MasterBody | PptPlaceholderType::Body => (p::PlaceholderValues::Body, None),
    PptPlaceholderType::MasterCenterTitle | PptPlaceholderType::CenterTitle => {
      (p::PlaceholderValues::CenteredTitle, None)
    }
    PptPlaceholderType::MasterSubTitle | PptPlaceholderType::SubTitle => {
      (p::PlaceholderValues::SubTitle, None)
    }
    PptPlaceholderType::MasterNotesSlideImage | PptPlaceholderType::NotesSlideImage => {
      (p::PlaceholderValues::SlideImage, None)
    }
    PptPlaceholderType::MasterNotesBody | PptPlaceholderType::NotesBody => {
      (p::PlaceholderValues::Body, None)
    }
    PptPlaceholderType::MasterDate => (p::PlaceholderValues::DateAndTime, None),
    PptPlaceholderType::MasterSlideNumber => (p::PlaceholderValues::SlideNumber, None),
    PptPlaceholderType::MasterFooter => (p::PlaceholderValues::Footer, None),
    PptPlaceholderType::MasterHeader => (p::PlaceholderValues::Header, None),
    PptPlaceholderType::VerticalTitle => (
      p::PlaceholderValues::Title,
      Some(p::DirectionValues::Vertical),
    ),
    PptPlaceholderType::VerticalBody => (
      p::PlaceholderValues::Body,
      Some(p::DirectionValues::Vertical),
    ),
    PptPlaceholderType::Object => (p::PlaceholderValues::Object, None),
    PptPlaceholderType::Graph => (p::PlaceholderValues::Chart, None),
    PptPlaceholderType::Table => (p::PlaceholderValues::Table, None),
    PptPlaceholderType::ClipArt => (p::PlaceholderValues::ClipArt, None),
    PptPlaceholderType::OrganizationChart => (p::PlaceholderValues::Diagram, None),
    PptPlaceholderType::Media => (p::PlaceholderValues::Media, None),
    PptPlaceholderType::VerticalObject => (
      p::PlaceholderValues::Object,
      Some(p::DirectionValues::Vertical),
    ),
    PptPlaceholderType::Picture => (p::PlaceholderValues::Picture, None),
    PptPlaceholderType::Compatibility(_) => return None,
  };
  let size = match source.size {
    PptPlaceholderSize::Full => p::PlaceholderSizeValues::Full,
    PptPlaceholderSize::Half => p::PlaceholderSizeValues::Half,
    PptPlaceholderSize::Quarter => p::PlaceholderSizeValues::Quarter,
    PptPlaceholderSize::Compatibility(_) => return None,
  };
  Some(p::PlaceholderShape {
    r#type: Some(r#type),
    orientation,
    size: Some(size),
    index: u32::try_from(source.position).ok(),
    ..Default::default()
  })
}

const fn native_shape_type(value: u16) -> Option<a::ShapeTypeValues> {
  Some(match value {
    1 | 24..=31 | 136..=175 | 202 => a::ShapeTypeValues::Rectangle,
    2 => a::ShapeTypeValues::RoundRectangle,
    3 => a::ShapeTypeValues::Ellipse,
    4 => a::ShapeTypeValues::Diamond,
    5 => a::ShapeTypeValues::Triangle,
    6 => a::ShapeTypeValues::RightTriangle,
    7 => a::ShapeTypeValues::Parallelogram,
    8 => a::ShapeTypeValues::Trapezoid,
    9 => a::ShapeTypeValues::Hexagon,
    10 => a::ShapeTypeValues::Octagon,
    11 => a::ShapeTypeValues::Plus,
    12 => a::ShapeTypeValues::Star5,
    13 => a::ShapeTypeValues::RightArrow,
    15 => a::ShapeTypeValues::HomePlate,
    16 => a::ShapeTypeValues::Cube,
    18 => a::ShapeTypeValues::Star16,
    19 => a::ShapeTypeValues::Arc,
    20 => a::ShapeTypeValues::Line,
    21 => a::ShapeTypeValues::Plaque,
    22 => a::ShapeTypeValues::Can,
    23 => a::ShapeTypeValues::Donut,
    32 => a::ShapeTypeValues::StraightConnector1,
    33 => a::ShapeTypeValues::BentConnector2,
    34 => a::ShapeTypeValues::BentConnector3,
    35 => a::ShapeTypeValues::BentConnector4,
    36 => a::ShapeTypeValues::BentConnector5,
    37 => a::ShapeTypeValues::CurvedConnector2,
    38 => a::ShapeTypeValues::CurvedConnector3,
    39 => a::ShapeTypeValues::CurvedConnector4,
    40 => a::ShapeTypeValues::CurvedConnector5,
    55 => a::ShapeTypeValues::Chevron,
    56 => a::ShapeTypeValues::Pentagon,
    57 => a::ShapeTypeValues::NoSmoking,
    58 => a::ShapeTypeValues::Star8,
    59 => a::ShapeTypeValues::Star16,
    60 => a::ShapeTypeValues::Star32,
    65 => a::ShapeTypeValues::FoldedCorner,
    66 => a::ShapeTypeValues::LeftArrow,
    67 => a::ShapeTypeValues::DownArrow,
    68 => a::ShapeTypeValues::UpArrow,
    69 => a::ShapeTypeValues::LeftRightArrow,
    70 => a::ShapeTypeValues::UpDownArrow,
    73 => a::ShapeTypeValues::LightningBolt,
    74 => a::ShapeTypeValues::Heart,
    75 => a::ShapeTypeValues::Frame,
    76 => a::ShapeTypeValues::QuadArrow,
    84 => a::ShapeTypeValues::Bevel,
    85 => a::ShapeTypeValues::LeftBracket,
    86 => a::ShapeTypeValues::RightBracket,
    87 => a::ShapeTypeValues::LeftBrace,
    88 => a::ShapeTypeValues::RightBrace,
    95 => a::ShapeTypeValues::BlockArc,
    96 => a::ShapeTypeValues::SmileyFace,
    97 => a::ShapeTypeValues::VerticalScroll,
    98 => a::ShapeTypeValues::HorizontalScroll,
    99 => a::ShapeTypeValues::CircularArrow,
    101 => a::ShapeTypeValues::UTurnArrow,
    102 => a::ShapeTypeValues::CurvedRightArrow,
    103 => a::ShapeTypeValues::CurvedLeftArrow,
    104 => a::ShapeTypeValues::CurvedUpArrow,
    105 => a::ShapeTypeValues::CurvedDownArrow,
    106 => a::ShapeTypeValues::CloudCallout,
    107 => a::ShapeTypeValues::EllipseRibbon,
    108 => a::ShapeTypeValues::EllipseRibbon2,
    109 => a::ShapeTypeValues::FlowChartProcess,
    110 => a::ShapeTypeValues::FlowChartDecision,
    111 => a::ShapeTypeValues::FlowChartInputOutput,
    112 => a::ShapeTypeValues::FlowChartPredefinedProcess,
    113 => a::ShapeTypeValues::FlowChartInternalStorage,
    114 => a::ShapeTypeValues::FlowChartDocument,
    115 => a::ShapeTypeValues::FlowChartMultidocument,
    116 => a::ShapeTypeValues::FlowChartTerminator,
    117 => a::ShapeTypeValues::FlowChartPreparation,
    118 => a::ShapeTypeValues::FlowChartManualInput,
    119 => a::ShapeTypeValues::FlowChartManualOperation,
    120 => a::ShapeTypeValues::FlowChartConnector,
    121 => a::ShapeTypeValues::FlowChartPunchedCard,
    122 => a::ShapeTypeValues::FlowChartPunchedTape,
    123 => a::ShapeTypeValues::FlowChartSummingJunction,
    124 => a::ShapeTypeValues::FlowChartOr,
    125 => a::ShapeTypeValues::FlowChartCollate,
    126 => a::ShapeTypeValues::FlowChartSort,
    127 => a::ShapeTypeValues::FlowChartExtract,
    128 => a::ShapeTypeValues::FlowChartMerge,
    129 => a::ShapeTypeValues::FlowChartOfflineStorage,
    130 => a::ShapeTypeValues::FlowChartOnlineStorage,
    131 => a::ShapeTypeValues::FlowChartMagneticTape,
    132 => a::ShapeTypeValues::FlowChartMagneticDisk,
    133 => a::ShapeTypeValues::FlowChartMagneticDrum,
    134 => a::ShapeTypeValues::FlowChartDisplay,
    135 => a::ShapeTypeValues::FlowChartDelay,
    176 => a::ShapeTypeValues::FlowChartAlternateProcess,
    177 => a::ShapeTypeValues::FlowChartOffpageConnector,
    183 => a::ShapeTypeValues::Sun,
    184 => a::ShapeTypeValues::Moon,
    185 => a::ShapeTypeValues::BracketPair,
    186 => a::ShapeTypeValues::BracePair,
    187 => a::ShapeTypeValues::Star4,
    188 => a::ShapeTypeValues::DoubleWave,
    189 => a::ShapeTypeValues::ActionButtonBlank,
    190 => a::ShapeTypeValues::ActionButtonHome,
    191 => a::ShapeTypeValues::ActionButtonHelp,
    192 => a::ShapeTypeValues::ActionButtonInformation,
    193 => a::ShapeTypeValues::ActionButtonForwardNext,
    194 => a::ShapeTypeValues::ActionButtonBackPrevious,
    195 => a::ShapeTypeValues::ActionButtonEnd,
    196 => a::ShapeTypeValues::ActionButtonBeginning,
    197 => a::ShapeTypeValues::ActionButtonReturn,
    198 => a::ShapeTypeValues::ActionButtonDocument,
    199 => a::ShapeTypeValues::ActionButtonSound,
    200 => a::ShapeTypeValues::ActionButtonMovie,
    _ => return None,
  })
}

fn master_to_emu(value: i32) -> Result<i64> {
  let product = i64::from(value)
    .checked_mul(3_175)
    .ok_or_else(|| olecfsdk::Error::Limit("PPT master coordinate overflow".into()))?;
  Ok(if product >= 0 {
    (product + 1) / 2
  } else {
    (product - 1) / 2
  })
}

fn append_text_body(
  source: PptLiveTextBodyRef<'_>,
  paragraphs: &mut Vec<a::Paragraph>,
  source_location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<()> {
  let mut choices = Vec::new();
  for atom in source.character_atoms() {
    match atom {
      PptLiveTextAtomRef::String { value, .. } => {
        append_text(
          value,
          &mut choices,
          paragraphs,
          source_location,
          options,
          report,
        )?;
      }
      PptLiveTextAtomRef::CompatibilityUtf16 { .. } => unsupported(
        report,
        options,
        ConversionCode::CompatibilityUtf16,
        source_location,
      )?,
    }
  }
  paragraphs.push(a::Paragraph {
    paragraph_choice: choices,
    ..Default::default()
  });
  Ok(())
}

fn append_text(
  value: &str,
  choices: &mut Vec<a::ParagraphChoice>,
  paragraphs: &mut Vec<a::Paragraph>,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<()> {
  let mut start = 0;
  for (index, character) in value.char_indices() {
    let mapped = match character {
      '\r' => Some(None),
      '\u{000b}' => Some(Some(a::ParagraphChoice::Break(Box::default()))),
      value if value.is_control() && !matches!(value, '\t' | '\n') => {
        unsupported(
          report,
          options,
          ConversionCode::ControlCharacterNotMapped,
          source,
        )?;
        Some(None)
      }
      _ => None,
    };
    let Some(mapped) = mapped else {
      continue;
    };
    push_run(choices, &value[start..index]);
    if character == '\r' {
      paragraphs.push(a::Paragraph {
        paragraph_choice: std::mem::take(choices),
        ..Default::default()
      });
    } else if let Some(mapped) = mapped {
      choices.push(mapped);
    }
    start = index + character.len_utf8();
  }
  push_run(choices, &value[start..]);
  Ok(())
}

fn push_run(choices: &mut Vec<a::ParagraphChoice>, value: &str) {
  if !value.is_empty() {
    choices.push(a::ParagraphChoice::Run(Box::new(a::Run {
      text: value.to_owned(),
      ..Default::default()
    })));
  }
}

fn unsupported(
  report: &mut ConversionReport,
  options: ConversionOptions,
  code: ConversionCode,
  source: SourceLocation,
) -> Result<()> {
  match options.unsupported {
    LossPolicy::Reject => Err(Error::Unsupported {
      code,
      location: source,
    }),
    LossPolicy::Report => {
      report.issue(Disposition::Unsupported, code, source);
      Ok(())
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn image_content_types_do_not_mislabel_raw_dib_as_bmp() {
    assert_eq!(ppt_image_content_type(OfficeArtImageFormat::Dib), None);
    assert_eq!(
      ppt_image_content_type(OfficeArtImageFormat::Png),
      Some("image/png")
    );
  }

  #[test]
  fn transition_direction_rules_match_ms_ppt() {
    let mut loss = false;
    assert!(matches!(
      ppt_transition_choice(1, 0xff, &mut loss),
      Some(p::TransitionChoice::RandomTransition)
    ));
    assert!(!loss, "Random ignores its direction byte");

    assert!(ppt_transition_choice(255, 0xff, &mut loss).is_none());
    assert!(!loss, "undefined 0xFF is ignored by specification");

    let Some(p::TransitionChoice::BlindsTransition(blinds)) =
      ppt_transition_choice(2, 0, &mut loss)
    else {
      panic!("Blinds maps to a typed transition")
    };
    assert_eq!(blinds.direction, Some(p::DirectionValues::Vertical));
    let Some(p::TransitionChoice::CheckerTransition(checker)) =
      ppt_transition_choice(3, 0, &mut loss)
    else {
      panic!("Checker maps to a typed transition")
    };
    assert_eq!(checker.direction, Some(p::DirectionValues::Horizontal));
    assert!(!loss);
  }

  #[test]
  fn transition_ignores_reserved_storage_but_reports_real_unmapped_behavior() {
    let ignored_storage = SlideShowSlideInfoAtom {
      slide_time: -1,
      sound_id_ref: 42,
      effect_direction: 0,
      effect_type: 1,
      flags: 0b1010_1010_1010_1010,
      speed: 1,
      unused: [1, 2, 3],
    };
    // Clear the semantic sound/auto/cursor flags while retaining every
    // reserved bit that MS-PPT says consumers MUST ignore.
    let ignored_storage = SlideShowSlideInfoAtom {
      flags: ignored_storage.flags & !((1 << 4) | (1 << 6) | (1 << 8) | (1 << 10) | (1 << 12)),
      ..ignored_storage
    };
    assert!(!ppt_transition_has_unmapped_features(&ignored_storage));

    assert!(ppt_transition_has_unmapped_features(
      &SlideShowSlideInfoAtom {
        flags: 1 << 4,
        ..ignored_storage
      }
    ));
    assert!(ppt_transition_has_unmapped_features(
      &SlideShowSlideInfoAtom {
        slide_time: 86_399_001,
        flags: 1 << 10,
        ..ignored_storage
      }
    ));
  }
}
