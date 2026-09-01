use olecfsdk::doc::{
  DocBlockRef, DocCommentRef, DocComments, DocCp, DocCpRange, DocDataNodeValue, DocDocumentPartRef,
  DocFieldRef, DocFile, DocNoteKind, DocNotes, DocOfficeArtColor, DocOfficeArtFill,
  DocOfficeArtImageLink, DocOfficeArtLine, DocOfficeArtShapeRef, DocOfficeArtTextInsets,
  DocParagraphRef, DocSectionProperties, DocShapeAnchorRef, DocSpecialContentRef, DocTableCellRef,
  DocTableRef, DocTables, DocTextPieceValueRef, DocTextRangeRef, DocTextboxes, FieldDocumentPart,
  FieldEndFlags, GrpPrl, KnownSprm, PicfAndOfficeArtData, Prl, PrmPropertiesRef, Sprm, SprmGroup,
  SprmKind, SprmOperand, SprmOperandSize, StyleFormatting, StyleGeneralFlags, StyleKind,
};
use olecfsdk::office_art::{
  OfficeArtImageFormat, OfficeArtImageRef, OfficeArtMetafileData, OfficeArtRecordData,
};
use ooxmlsdk::{
  common::XmlNamespace,
  namespaces::XmlKnownNamespace,
  parts::{
    endnotes_part::EndnotesPart, footnotes_part::FootnotesPart,
    main_document_part::MainDocumentPart, style_definitions_part::StyleDefinitionsPart,
    wordprocessing_comments_part::WordprocessingCommentsPart,
    wordprocessing_document::WordprocessingDocument,
  },
  schemas::{
    schemas_microsoft_com_office_word_2010_wordprocessing_shape as wps,
    schemas_openxmlformats_org_drawingml_2006_main as a,
    schemas_openxmlformats_org_drawingml_2006_picture as pic,
    schemas_openxmlformats_org_drawingml_2006_wordprocessing_drawing as wp,
    schemas_openxmlformats_org_wordprocessingml_2006_main::{
      AdjustRightIndent, AutoRedefine, AutoSpaceDe, AutoSpaceDn, BasedOn, BiDi, Body, BodyChoice,
      Bold, BoldComplexScript, BookmarkEnd, BookmarkStart, Caps, Columns, Comment, CommentChoice,
      CommentRangeEnd, CommentRangeStart, CommentReference, Comments, ContextualSpacing, Document,
      DoubleStrike, Drawing, DrawingChoice, Emboss, Endnote, EndnoteChoice, EndnoteReference,
      Endnotes, FieldChar, FieldCharValues, FieldCode, FontSize, FontSizeComplexScript, Footnote,
      FootnoteChoice, FootnoteReference, Footnotes, GutterOnRight, Imprint, Indentation, Italic,
      ItalicComplexScript, Justification, JustificationValues, KeepLines, KeepNext, Kinsoku,
      LineSpacingRuleValues, LinkedStyle, Locked, MirrorIndents, NextParagraphStyle, NoProof,
      Outline, OutlineLevel, OverflowPunctuation, PageBreakBefore, PageMargin, PageNumberType,
      PageOrientationValues, PageSize, Paragraph, ParagraphChoice, ParagraphProperties,
      ParagraphStyleId, Personal, PersonalCompose, PersonalReply, PrimaryStyle, RightToLeftText,
      Run, RunChoice, RunProperties, RunPropertiesChoice, SectionMarkValues, SectionProperties,
      SectionType, SemiHidden, Shadow, SmallCaps, SnapToGrid, SpacingBetweenLines, Strike, Style,
      StyleHidden, StyleName, StyleParagraphProperties, StyleRunProperties, StyleValues, Styles,
      SuppressAutoHyphens, SuppressLineNumbers, SuppressOverlap, Table, TableCell, TableCellChoice,
      TableChoice2, TableRow, TableRowChoice, Text, TextBoxContent, TextBoxContentChoice, TextType,
      TitlePage, TopLinePunctuation, UiPriority, Underline, UnderlineValues, UnhideWhenUsed,
      Vanish, WebHidden, WidowControl,
    },
    xml::SpaceProcessingModeValues,
  },
  sdk::WordprocessingDocumentType,
  simple_type::{HpsMeasureValue, OnOffValue, SignedTwipsMeasureValue, TwipsMeasureValue},
};

use crate::{
  ConversionCode, ConversionOptions, ConversionOutput, ConversionReport, Disposition, Error,
  LossPolicy, Result, SourceLocation, metadata::convert_core_properties,
};

macro_rules! set_on_off {
  ($target:expr, $element:ident, $value:expr) => {
    match $value {
      Some(value) => {
        $target = Some($element {
          val: Some(OnOffValue::from_bool(value)),
        });
        true
      }
      None => false,
    }
  };
}

struct PendingImage<'a> {
  relationship_id: String,
  content_type: &'static str,
  data: &'a [u8],
}

#[derive(Clone, Copy)]
struct DocFloatingShape<'a> {
  anchor: DocShapeAnchorRef<'a>,
  story_index: Option<usize>,
  chain_index: Option<u16>,
  story_text: Option<DocTextRangeRef<'a>>,
  has_flow_breaks: bool,
}

struct DocMediaState<'a> {
  pending: Vec<PendingImage<'a>>,
  next_drawing_id: u32,
  floating_shapes: Vec<DocFloatingShape<'a>>,
  textbox_fields: Option<DocFieldCursor>,
  source: Option<&'a DocFile>,
}

#[derive(Clone, Copy)]
struct FloatingGeometry {
  left: i32,
  top: i32,
  width: i32,
  height: i32,
  reverse_horizontal: bool,
  reverse_vertical: bool,
}

struct FloatingWordprocessingStyle {
  fill: Option<wps::ShapePropertiesChoice2>,
  outline: Option<Box<a::Outline>>,
  insets: DocOfficeArtTextInsets,
}

struct FloatingPictureStyle {
  fill: Option<pic::ShapePropertiesChoice2>,
  outline: Option<Box<a::Outline>>,
  source_rectangle: Option<a::SourceRectangle>,
}

impl Default for DocMediaState<'_> {
  fn default() -> Self {
    Self {
      pending: Vec::new(),
      next_drawing_id: 1,
      floating_shapes: Vec::new(),
      textbox_fields: None,
      source: None,
    }
  }
}

/// Converts a typed Word 97–2003 root into a WordprocessingML package.
///
/// The default policy rejects the first known semantic loss. Use
/// [`convert_doc_with_options`] to request an explicit diagnostic report.
pub fn convert_doc(source: &DocFile) -> Result<ConversionOutput<WordprocessingDocument>> {
  convert_doc_with_options(source, ConversionOptions::default())
}

/// Converts a typed Word 97–2003 root with an explicit loss policy.
pub fn convert_doc_with_options(
  source: &DocFile,
  options: ConversionOptions,
) -> Result<ConversionOutput<WordprocessingDocument>> {
  // Build the relationship graph in compatibility mode so malformed joins
  // become typed conversion diagnostics. The caller's LossPolicy still
  // rejects them by default; Report mode can preserve all independent,
  // well-formed source units instead of aborting the entire conversion.
  let tree = source.content_tree_compatible()?;
  let main = tree
    .part(FieldDocumentPart::Main)
    .expect("the DOC content tree always contains the main document part");
  let mut report = ConversionReport::default();
  let mut media = DocMediaState {
    source: Some(source),
    ..Default::default()
  };
  report.record(Disposition::Mapped);
  let styles = convert_styles(source, options, &mut report)?;
  let sections = match main.sections() {
    Ok(sections) => Some(sections),
    Err(_) if options.unsupported == LossPolicy::Report => {
      report.issue(
        Disposition::Unsupported,
        ConversionCode::SectionBoundaryNotMapped,
        location(main.part(), main.local_cp_range()),
      );
      None
    }
    Err(error) => return Err(error.into()),
  };
  let mut converted_sections = Vec::with_capacity(
    sections
      .as_ref()
      .map_or(0, |sections| sections.sections().len()),
  );
  for section in sections
    .as_ref()
    .into_iter()
    .flat_map(|sections| sections.sections())
  {
    let (converted, has_unmapped) = convert_section(section.properties());
    if has_unmapped {
      unsupported(
        &mut report,
        options,
        ConversionCode::SectionFormattingNotMapped,
        SourceLocation::DocSection {
          section_index: section.section_index(),
          start_cp: section.local_cp_range().start.value(),
          end_cp: section.local_cp_range().end.value(),
        },
      )?;
    }
    converted_sections.push(Some(converted));
    report.record(Disposition::Mapped);
  }

  let footnotes = tree.footnotes()?;
  let endnotes = tree.endnotes()?;
  let comments = tree.comments()?;
  let textboxes = tree.main_textboxes()?;
  let note_references = collect_note_references(&footnotes, &endnotes, options, &mut report)?;
  let comment_references = collect_comment_references(&comments);
  for diagnostic in comments.diagnostics() {
    unsupported(
      &mut report,
      options,
      ConversionCode::CommentRelationshipNotMapped,
      diagnostic
        .index
        .and_then(|index| comments.comments().get(index).copied())
        .map_or(SourceLocation::Document, comment_location),
    )?;
  }
  for diagnostic in textboxes.diagnostics() {
    unsupported(
      &mut report,
      options,
      ConversionCode::TextboxRelationshipNotMapped,
      diagnostic
        .index
        .and_then(|index| textboxes.anchors().get(index).copied())
        .map_or(SourceLocation::Document, textbox_anchor_location),
    )?;
  }
  media.floating_shapes = collect_floating_shapes(&textboxes);
  media.textbox_fields = tree
    .part(FieldDocumentPart::Textbox)
    .map(|part| DocFieldCursor::new(collect_document_field_spans(part)));

  for part in tree.parts() {
    if !matches!(
      part.part(),
      FieldDocumentPart::Main
        | FieldDocumentPart::Footnote
        | FieldDocumentPart::Endnote
        | FieldDocumentPart::Comment
        | FieldDocumentPart::Textbox
    ) && !part.local_cp_range().is_empty()
    {
      unsupported(
        &mut report,
        options,
        ConversionCode::NonMainStoryNotMapped,
        location(part.part(), part.local_cp_range()),
      )?;
    }
  }

  let mut body_choice = Vec::new();
  let main_tables = main.tables()?;
  let mut main_fields = DocFieldCursor::new(collect_document_field_spans(main));
  let mut range_events = collect_document_bookmark_events(&tree, options, &mut report)?;
  range_events.extend(collect_comment_range_events(&comments));
  sort_range_events(&mut range_events);
  let mut main_ranges = DocRangeCursor::new(range_events);
  let mut main_flow = DocFlowState {
    fields: &mut main_fields,
    ranges: &mut main_ranges,
    note_references: &note_references,
    comment_references: &comment_references,
  };
  let intermediate_sections = sections
    .as_ref()
    .map(|sections| sections.sections())
    .and_then(|sections| sections.get(..sections.len().saturating_sub(1)))
    .unwrap_or_default();
  let mut next_section = 0;
  for block in main.blocks_with_tables(&main_tables)?.blocks() {
    match block {
      DocBlockRef::Paragraph(paragraph) => {
        let mut converted = convert_paragraph(
          *paragraph,
          ParagraphContext::Body,
          &mut main_flow,
          options,
          &mut report,
          &mut media,
        )?;
        if intermediate_sections
          .get(next_section)
          .is_some_and(|section| section.local_cp_range().end == paragraph.local_cp_range().end)
        {
          converted
            .paragraph_properties
            .get_or_insert_with(Box::default)
            .section_properties = converted_sections
            .get_mut(next_section)
            .and_then(Option::take)
            .map(Box::new);
          next_section += 1;
          report.record(Disposition::Mapped);
        }
        body_choice.push(BodyChoice::Paragraph(Box::new(converted)));
      }
      DocBlockRef::Table(table) => {
        while intermediate_sections
          .get(next_section)
          .is_some_and(|section| section.local_cp_range().end <= table.local_cp_range().end)
        {
          let section = intermediate_sections[next_section];
          unsupported(
            &mut report,
            options,
            ConversionCode::SectionBoundaryNotMapped,
            SourceLocation::DocSection {
              section_index: section.section_index(),
              start_cp: section.local_cp_range().start.value(),
              end_cp: section.local_cp_range().end.value(),
            },
          )?;
          next_section += 1;
        }
        body_choice.push(BodyChoice::Table(Box::new(convert_table(
          table,
          &main_tables,
          ParagraphContext::TableCell,
          &mut main_flow,
          options,
          &mut report,
          &mut media,
        )?)));
      }
    }
  }
  while let Some(section) = intermediate_sections.get(next_section) {
    unsupported(
      &mut report,
      options,
      ConversionCode::SectionBoundaryNotMapped,
      SourceLocation::DocSection {
        section_index: section.section_index(),
        start_cp: section.local_cp_range().start.value(),
        end_cp: section.local_cp_range().end.value(),
      },
    )?;
    next_section += 1;
  }
  main_flow.ranges.finish(options, &mut report)?;
  let final_section_properties = converted_sections
    .last_mut()
    .and_then(Option::take)
    .map(Box::new);

  let mut document = WordprocessingDocument::create(WordprocessingDocumentType::Document);
  let main_part = document.add_main_document_part()?;
  main_part.set_root_element(
    &mut document,
    Document {
      xmlns: vec![
        XmlNamespace::known(XmlKnownNamespace::W),
        XmlNamespace::known(XmlKnownNamespace::R),
        XmlNamespace::known(XmlKnownNamespace::A),
        XmlNamespace::known(XmlKnownNamespace::Pic),
        XmlNamespace::known(XmlKnownNamespace::Wp),
        XmlNamespace::known(XmlKnownNamespace::Wps),
      ],
      body: Some(Box::new(Body {
        body_choice,
        section_properties: final_section_properties,
      })),
      ..Default::default()
    },
  )?;
  convert_notes_parts(
    &footnotes,
    &endnotes,
    &main_part,
    &mut document,
    options,
    &mut report,
    &mut media,
  )?;
  convert_comments_part(
    &comments,
    &main_part,
    &mut document,
    options,
    &mut report,
    &mut media,
  )?;
  if let Some(styles) = styles {
    let styles_part = main_part.add_new_part_auto_id::<_, StyleDefinitionsPart>(&mut document)?;
    styles_part.set_root_element(&mut document, styles)?;
  }
  for image in media.pending {
    let image_part =
      main_part.add_image_part_with_id(&mut document, image.content_type, image.relationship_id)?;
    image_part.set_data(&mut document, image.data.to_vec())?;
    report.record(Disposition::Mapped);
  }
  if let Some(properties) = convert_core_properties(&source.shared, options, &mut report)? {
    let properties_part = document.add_core_file_properties_part()?;
    properties_part.set_root_element(&mut document, properties)?;
  }

  Ok(ConversionOutput { document, report })
}

fn convert_section(source: &DocSectionProperties) -> (SectionProperties, bool) {
  let Some(sepx) = source.value.as_ref() else {
    return (SectionProperties::default(), false);
  };
  let mut target = SectionProperties::default();
  let mut has_unmapped = sepx.trailing_byte.is_some();
  let mut restart_page_number = None;
  let mut page_number_97 = None;
  let mut page_number = None;
  for property in &sepx.properties.properties {
    let mapped = match property.sprm.kind() {
      SprmKind::Known(KnownSprm::SBkc) => {
        let Some(value) = byte(&property.operand).and_then(section_mark) else {
          has_unmapped = true;
          continue;
        };
        target.section_type = Some(SectionType { val: value });
        true
      }
      SprmKind::Known(KnownSprm::SFTitlePage) => {
        set_on_off!(target.title_page, TitlePage, bool8(&property.operand))
      }
      SprmKind::Known(KnownSprm::SFEvenlySpaced) => {
        let Some(value) = bool8(&property.operand) else {
          has_unmapped = true;
          continue;
        };
        target
          .columns
          .get_or_insert_with(Columns::default)
          .equal_width = Some(OnOffValue::from_bool(value));
        true
      }
      SprmKind::Known(KnownSprm::SCcolumns) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 43) else {
          has_unmapped = true;
          continue;
        };
        target
          .columns
          .get_or_insert_with(Columns::default)
          .column_count = Some(i16::try_from(value + 1).expect("section columns fit i16"));
        true
      }
      SprmKind::Known(KnownSprm::SDxaColumns) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target.columns.get_or_insert_with(Columns::default).space =
          Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SLBetween) => {
        let Some(value) = bool8(&property.operand) else {
          has_unmapped = true;
          continue;
        };
        target
          .columns
          .get_or_insert_with(Columns::default)
          .separator = Some(OnOffValue::from_bool(value));
        true
      }
      SprmKind::Known(KnownSprm::SBOrientation) => {
        let Some(value) = byte(&property.operand).and_then(page_orientation) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_size
          .get_or_insert_with(PageSize::default)
          .orient = Some(value);
        true
      }
      SprmKind::Known(KnownSprm::SXaPage) => {
        let Some(value) =
          word_u16(&property.operand).filter(|value| (144..=31_680).contains(value))
        else {
          has_unmapped = true;
          continue;
        };
        target.page_size.get_or_insert_with(PageSize::default).width =
          Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SYaPage) => {
        let Some(value) =
          word_u16(&property.operand).filter(|value| (144..=31_680).contains(value))
        else {
          has_unmapped = true;
          continue;
        };
        target
          .page_size
          .get_or_insert_with(PageSize::default)
          .height = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDxaLeft) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .left = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDxaRight) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .right = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDyaTop) => {
        let Some(value) =
          word_i16(&property.operand).filter(|value| (-31_665..=31_665).contains(value))
        else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .top = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDyaBottom) => {
        let Some(value) =
          word_i16(&property.operand).filter(|value| (-31_665..=31_665).contains(value))
        else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .bottom = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDyaHdrTop) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .header = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDyaHdrBottom) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .footer = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SDzaGutter) => {
        let Some(value) = word_u16(&property.operand).filter(|value| *value <= 31_680) else {
          has_unmapped = true;
          continue;
        };
        target
          .page_margin
          .get_or_insert_with(PageMargin::default)
          .gutter = Some(TwipsMeasureValue::Twips(u64::from(value)));
        true
      }
      SprmKind::Known(KnownSprm::SFBiDi) => {
        set_on_off!(target.bi_di, BiDi, bool8(&property.operand))
      }
      SprmKind::Known(KnownSprm::SFRTLGutter) => set_on_off!(
        target.gutter_on_right,
        GutterOnRight,
        bool8(&property.operand)
      ),
      SprmKind::Known(KnownSprm::SFPgnRestart) => {
        restart_page_number = bool8(&property.operand);
        restart_page_number.is_some()
      }
      SprmKind::Known(KnownSprm::SPgnStart97) => {
        page_number_97 = word_u16(&property.operand).filter(|value| *value <= 32_766);
        page_number_97.is_some()
      }
      SprmKind::Known(KnownSprm::SPgnStart) => {
        page_number = dword_u32(&property.operand).filter(|value| *value <= 2_147_483_646);
        page_number.is_some()
      }
      _ => false,
    };
    if !mapped {
      has_unmapped = true;
    }
  }
  if restart_page_number == Some(true)
    && let Some(value) = page_number.or_else(|| page_number_97.map(u32::from))
  {
    target.page_number_type = Some(PageNumberType {
      start: Some(i32::try_from(value).expect("validated page number fits i32")),
      ..Default::default()
    });
  }
  (target, has_unmapped)
}

const fn section_mark(value: u8) -> Option<SectionMarkValues> {
  match value {
    0 => Some(SectionMarkValues::Continuous),
    1 => Some(SectionMarkValues::NextColumn),
    2 => Some(SectionMarkValues::NextPage),
    3 => Some(SectionMarkValues::EvenPage),
    4 => Some(SectionMarkValues::OddPage),
    _ => None,
  }
}

const fn page_orientation(value: u8) -> Option<PageOrientationValues> {
  match value {
    1 => Some(PageOrientationValues::Portrait),
    2 => Some(PageOrientationValues::Landscape),
    _ => None,
  }
}

#[derive(Clone, Copy)]
struct DocCommentReference {
  cp: u32,
  id: usize,
}

fn collect_comment_references(comments: &DocComments<'_>) -> Vec<DocCommentReference> {
  comments
    .comments()
    .iter()
    .map(|comment| DocCommentReference {
      cp: comment.reference_cp().value(),
      id: comment.index(),
    })
    .collect()
}

fn comment_location(comment: DocCommentRef<'_>) -> SourceLocation {
  let text = comment.text().local_cp_range();
  let selection = comment.commented_text().local_cp_range();
  SourceLocation::DocComment {
    comment_index: comment.index(),
    reference_cp: comment.reference_cp().value(),
    start_cp: text.start.value(),
    end_cp: text.end.value(),
    selection_start_cp: selection.start.value(),
    selection_end_cp: selection.end.value(),
  }
}

fn collect_comment_range_events(comments: &DocComments<'_>) -> Vec<DocRangeEvent> {
  let mut events = Vec::with_capacity(comments.comments().len().saturating_mul(2));
  for comment in comments.comments() {
    let range = comment.commented_text().local_cp_range();
    let source = comment_location(*comment);
    events.push(DocRangeEvent {
      cp: range.start.value(),
      id: comment.index(),
      source,
      boundary_code: ConversionCode::CommentBoundaryNotMapped,
      marker: DocRangeMarker::CommentStart,
    });
    events.push(DocRangeEvent {
      cp: range.end.value(),
      id: comment.index(),
      source,
      boundary_code: ConversionCode::CommentBoundaryNotMapped,
      marker: DocRangeMarker::CommentEnd,
    });
  }
  events
}

fn collect_floating_shapes<'a>(textboxes: &DocTextboxes<'a>) -> Vec<DocFloatingShape<'a>> {
  let mut shapes = Vec::with_capacity(textboxes.anchors().len());
  for anchor in textboxes.anchors() {
    let linked = anchor.shape().and_then(|shape| {
      textboxes.stories().iter().find_map(|story| {
        story
          .shapes()
          .iter()
          .find(|candidate| candidate.shape().shape_id == shape.shape().shape_id)
          .and_then(|candidate| {
            candidate
              .textbox_link()
              .map(|link| (story, link.chain_index()))
          })
      })
    });
    shapes.push(DocFloatingShape {
      anchor: *anchor,
      story_index: linked.map(|(story, _)| story.index()),
      chain_index: linked.map(|(_, chain_index)| chain_index),
      story_text: linked.map(|(story, _)| story.text()),
      has_flow_breaks: linked.is_some_and(|(story, _)| {
        story
          .breaks()
          .iter()
          .any(|value| value.source().text_overflow)
      }),
    });
  }
  shapes.sort_unstable_by_key(|shape| shape.anchor.anchor_cp().value());
  shapes
}

fn textbox_anchor_location(anchor: DocShapeAnchorRef<'_>) -> SourceLocation {
  SourceLocation::DocTextbox {
    document_part: anchor
      .shape()
      .map_or(olecfsdk::doc::TextboxDocumentPart::Main, |shape| {
        shape.document_part()
      }),
    story_index: None,
    shape_id: anchor.source().shape_id,
    anchor_cp: anchor.anchor_cp().value(),
    start_cp: None,
    end_cp: None,
  }
}

fn floating_shape_location(shape: DocFloatingShape<'_>) -> SourceLocation {
  let range = shape.story_text.map(DocTextRangeRef::local_cp_range);
  SourceLocation::DocTextbox {
    document_part: shape
      .anchor
      .shape()
      .map_or(olecfsdk::doc::TextboxDocumentPart::Main, |value| {
        value.document_part()
      }),
    story_index: shape.story_index,
    shape_id: shape.anchor.source().shape_id,
    anchor_cp: shape.anchor.anchor_cp().value(),
    start_cp: range.map(|range| range.start.value()),
    end_cp: range.map(|range| range.end.value()),
  }
}

#[derive(Clone, Copy)]
struct DocNoteReference {
  cp: u32,
  id: i64,
  kind: DocNoteKind,
}

fn collect_note_references(
  footnotes: &DocNotes<'_>,
  endnotes: &DocNotes<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Vec<DocNoteReference>> {
  let mut references = Vec::with_capacity(footnotes.notes().len() + endnotes.notes().len());
  for note in footnotes.notes().iter().chain(endnotes.notes()) {
    let id = i64::try_from(note.index() + 1)
      .map_err(|_| olecfsdk::Error::Limit("DOC note index exceeds i64".into()))?;
    if id > 32_767 {
      return Err(
        olecfsdk::Error::Limit("DOC note index exceeds OOXML note ID range".into()).into(),
      );
    }
    if !note.is_automatically_numbered() {
      unsupported(
        report,
        options,
        ConversionCode::NoteCustomMarkNotMapped,
        note_location(*note),
      )?;
      continue;
    }
    references.push(DocNoteReference {
      cp: note.reference_cp().value(),
      id,
      kind: note.kind(),
    });
  }
  references.sort_unstable_by_key(|reference| reference.cp);
  Ok(references)
}

fn note_location(note: olecfsdk::doc::DocNoteRef<'_>) -> SourceLocation {
  let range = note.text().local_cp_range();
  SourceLocation::DocNote {
    kind: note.kind(),
    note_index: note.index(),
    reference_cp: note.reference_cp().value(),
    start_cp: range.start.value(),
    end_cp: range.end.value(),
  }
}

fn convert_notes_parts<'a>(
  footnotes: &DocNotes<'a>,
  endnotes: &DocNotes<'a>,
  main_part: &MainDocumentPart,
  document: &mut WordprocessingDocument,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<()> {
  if !footnotes.notes().is_empty() {
    let mut fields = DocFieldCursor::new(collect_document_field_spans(
      footnotes.notes()[0].text().document_part(),
    ));
    let mut converted = Vec::with_capacity(footnotes.notes().len());
    for note in footnotes.notes() {
      if !note.is_automatically_numbered() {
        continue;
      }
      converted.push(Footnote {
        id: note_id(*note)?,
        footnote_choice: convert_note_blocks(
          *note,
          ParagraphContext::Footnote,
          ParagraphContext::FootnoteTableCell,
          &mut fields,
          options,
          report,
          media,
        )?
        .into_iter()
        .map(|block| match block {
          ConvertedStoryBlock::Paragraph(paragraph) => FootnoteChoice::Paragraph(paragraph),
          ConvertedStoryBlock::Table(table) => FootnoteChoice::Table(table),
        })
        .collect(),
        ..Default::default()
      });
    }
    if !converted.is_empty() {
      let part = main_part.add_new_part_auto_id::<_, FootnotesPart>(document)?;
      part.set_root_element(
        document,
        Footnotes {
          xmlns: vec![XmlNamespace::known(XmlKnownNamespace::W)],
          footnote: converted,
          ..Default::default()
        },
      )?;
    }
  }
  if !endnotes.notes().is_empty() {
    let mut fields = DocFieldCursor::new(collect_document_field_spans(
      endnotes.notes()[0].text().document_part(),
    ));
    let mut converted = Vec::with_capacity(endnotes.notes().len());
    for note in endnotes.notes() {
      if !note.is_automatically_numbered() {
        continue;
      }
      converted.push(Endnote {
        id: note_id(*note)?,
        endnote_choice: convert_note_blocks(
          *note,
          ParagraphContext::Endnote,
          ParagraphContext::EndnoteTableCell,
          &mut fields,
          options,
          report,
          media,
        )?
        .into_iter()
        .map(|block| match block {
          ConvertedStoryBlock::Paragraph(paragraph) => EndnoteChoice::Paragraph(paragraph),
          ConvertedStoryBlock::Table(table) => EndnoteChoice::Table(table),
        })
        .collect(),
        ..Default::default()
      });
    }
    if !converted.is_empty() {
      let part = main_part.add_new_part_auto_id::<_, EndnotesPart>(document)?;
      part.set_root_element(
        document,
        Endnotes {
          xmlns: vec![XmlNamespace::known(XmlKnownNamespace::W)],
          endnote: converted,
          ..Default::default()
        },
      )?;
    }
  }
  Ok(())
}

fn note_id(note: olecfsdk::doc::DocNoteRef<'_>) -> Result<i64> {
  let id = i64::try_from(note.index() + 1)
    .map_err(|_| olecfsdk::Error::Limit("DOC note index exceeds i64".into()))?;
  if id > 32_767 {
    return Err(olecfsdk::Error::Limit("DOC note index exceeds OOXML note ID range".into()).into());
  }
  Ok(id)
}

enum ConvertedStoryBlock {
  Paragraph(Box<Paragraph>),
  Table(Box<Table>),
}

fn convert_note_blocks<'a>(
  note: olecfsdk::doc::DocNoteRef<'a>,
  context: ParagraphContext,
  table_cell_context: ParagraphContext,
  fields: &mut DocFieldCursor,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Vec<ConvertedStoryBlock>> {
  let text = note.text();
  let tables = text.tables()?;
  let mut ranges = DocRangeCursor::new(Vec::new());
  let mut flow = DocFlowState {
    fields,
    ranges: &mut ranges,
    note_references: &[],
    comment_references: &[],
  };
  let mut blocks = Vec::new();
  for block in text.blocks_with_tables(&tables)?.blocks() {
    let range = match block {
      DocBlockRef::Paragraph(paragraph) => paragraph.local_cp_range(),
      DocBlockRef::Table(table) => table.local_cp_range(),
    };
    if range.start < text.local_cp_range().start || range.end > text.local_cp_range().end {
      unsupported(
        report,
        options,
        ConversionCode::NoteBoundaryNotMapped,
        note_location(note),
      )?;
    }
    blocks.push(match block {
      DocBlockRef::Paragraph(paragraph) => ConvertedStoryBlock::Paragraph(Box::new(
        convert_paragraph(*paragraph, context, &mut flow, options, report, media)?,
      )),
      DocBlockRef::Table(table) => ConvertedStoryBlock::Table(Box::new(convert_table(
        table,
        &tables,
        table_cell_context,
        &mut flow,
        options,
        report,
        media,
      )?)),
    });
  }
  report.record(Disposition::Mapped);
  Ok(blocks)
}

fn convert_comments_part<'a>(
  comments: &DocComments<'a>,
  main_part: &MainDocumentPart,
  document: &mut WordprocessingDocument,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<()> {
  let Some(first) = comments.comments().first() else {
    return Ok(());
  };
  let mut fields = DocFieldCursor::new(collect_document_field_spans(first.text().document_part()));
  let mut converted = Vec::with_capacity(comments.comments().len());
  for comment in comments.comments() {
    let source = comment_location(*comment);
    let author = convert_comment_string(
      comment.author().unwrap_or_default(),
      255,
      source,
      options,
      report,
    )?;
    let initials = (!comment.initials().is_empty())
      .then(|| convert_comment_string(comment.initials(), 9, source, options, report))
      .transpose()?;
    let date = comment
      .extended()
      .and_then(|extended| comment_date_time(extended.modified));
    if let Some(extended) = comment.extended() {
      if !extended.modified.is_ignored() && date.is_none() {
        unsupported(
          report,
          options,
          ConversionCode::CommentMetadataNotMapped,
          source,
        )?;
      }
      if extended.depth != 0 || extended.parent_offset != 0 {
        unsupported(
          report,
          options,
          ConversionCode::CommentThreadNotMapped,
          source,
        )?;
      }
      if extended.ink {
        unsupported(report, options, ConversionCode::CommentInkNotMapped, source)?;
      }
    }
    converted.push(Comment {
      initials,
      author,
      date,
      id: comment.index().to_string(),
      comment_choice: convert_comment_content(*comment, &mut fields, options, report, media)?,
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  let part = main_part.add_new_part_auto_id::<_, WordprocessingCommentsPart>(document)?;
  part.set_root_element(
    document,
    Comments {
      xmlns: vec![XmlNamespace::known(XmlKnownNamespace::W)],
      comment: converted,
      ..Default::default()
    },
  )?;
  Ok(())
}

fn convert_comment_string(
  value: &[u16],
  max_chars: usize,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<String> {
  let Ok(mut value) = String::from_utf16(value) else {
    unsupported(
      report,
      options,
      ConversionCode::CommentMetadataNotMapped,
      source,
    )?;
    return Ok(String::new());
  };
  if let Some((limit, _)) = value.char_indices().nth(max_chars) {
    unsupported(
      report,
      options,
      ConversionCode::CommentMetadataNotMapped,
      source,
    )?;
    value.truncate(limit);
  }
  Ok(value)
}

fn comment_date_time(value: olecfsdk::doc::Dttm) -> Option<String> {
  if value.is_ignored() || value.day > days_in_month(value.year_offset, value.month) {
    return None;
  }
  Some(format!(
    "{:04}-{:02}-{:02}T{:02}:{:02}:00",
    1900u16 + value.year_offset,
    value.month,
    value.day,
    value.hour,
    value.minute
  ))
}

const fn days_in_month(year_offset: u16, month: u8) -> u8 {
  match month {
    1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
    4 | 6 | 9 | 11 => 30,
    2 if (1900 + year_offset).is_multiple_of(400)
      || ((1900 + year_offset).is_multiple_of(4) && !(1900 + year_offset).is_multiple_of(100)) =>
    {
      29
    }
    2 => 28,
    _ => 0,
  }
}

fn convert_comment_content<'a>(
  comment: DocCommentRef<'a>,
  fields: &mut DocFieldCursor,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Vec<CommentChoice>> {
  let text = comment.text();
  let tables = text.tables()?;
  let mut ranges = DocRangeCursor::new(Vec::new());
  let mut flow = DocFlowState {
    fields,
    ranges: &mut ranges,
    note_references: &[],
    comment_references: &[],
  };
  let mut result = Vec::new();
  for block in text.blocks_with_tables(&tables)?.blocks() {
    let range = match block {
      DocBlockRef::Paragraph(paragraph) => paragraph.local_cp_range(),
      DocBlockRef::Table(table) => table.local_cp_range(),
    };
    if range.start < text.local_cp_range().start || range.end > text.local_cp_range().end {
      unsupported(
        report,
        options,
        ConversionCode::CommentBoundaryNotMapped,
        comment_location(comment),
      )?;
    }
    result.push(match block {
      DocBlockRef::Paragraph(paragraph) => CommentChoice::Paragraph(Box::new(convert_paragraph(
        *paragraph,
        ParagraphContext::Comment,
        &mut flow,
        options,
        report,
        media,
      )?)),
      DocBlockRef::Table(table) => CommentChoice::Table(Box::new(convert_table(
        table,
        &tables,
        ParagraphContext::CommentTableCell,
        &mut flow,
        options,
        report,
        media,
      )?)),
    });
  }
  Ok(result)
}

struct DocFlowState<'cursor, 'source> {
  fields: &'cursor mut DocFieldCursor,
  ranges: &'cursor mut DocRangeCursor,
  note_references: &'source [DocNoteReference],
  comment_references: &'source [DocCommentReference],
}

enum DocRangeMarker {
  BookmarkStart {
    name: String,
    column_first: Option<i32>,
    column_last: Option<i32>,
  },
  BookmarkEnd,
  CommentStart,
  CommentEnd,
}

struct DocRangeEvent {
  cp: u32,
  id: usize,
  source: SourceLocation,
  boundary_code: ConversionCode,
  marker: DocRangeMarker,
}

struct DocRangeCursor {
  events: Vec<DocRangeEvent>,
  next: usize,
}

impl DocRangeCursor {
  fn new(events: Vec<DocRangeEvent>) -> Self {
    Self { events, next: 0 }
  }

  fn next_position(&self) -> Option<u32> {
    self.events.get(self.next).map(|event| event.cp)
  }

  fn next_source(&self) -> Option<SourceLocation> {
    self.events.get(self.next).map(|event| event.source)
  }

  fn emit_next(
    &mut self,
    paragraph_choice: &mut Vec<ParagraphChoice>,
    report: &mut ConversionReport,
  ) {
    let event = &mut self.events[self.next];
    let id = event.id.to_string();
    let choice = match &mut event.marker {
      DocRangeMarker::BookmarkStart {
        name,
        column_first,
        column_last,
      } => {
        report.record(Disposition::Mapped);
        ParagraphChoice::BookmarkStart(BookmarkStart {
          name: std::mem::take(name),
          column_first: *column_first,
          column_last: *column_last,
          id,
          ..Default::default()
        })
      }
      DocRangeMarker::BookmarkEnd => ParagraphChoice::BookmarkEnd(BookmarkEnd {
        id,
        ..Default::default()
      }),
      DocRangeMarker::CommentStart => {
        report.record(Disposition::Mapped);
        ParagraphChoice::CommentRangeStart(CommentRangeStart {
          id,
          ..Default::default()
        })
      }
      DocRangeMarker::CommentEnd => ParagraphChoice::CommentRangeEnd(CommentRangeEnd {
        id,
        ..Default::default()
      }),
    };
    paragraph_choice.push(choice);
    self.next += 1;
  }

  fn finish(&mut self, options: ConversionOptions, report: &mut ConversionReport) -> Result<()> {
    let mut previous_event = None;
    while let Some(event) = self.events.get(self.next) {
      let event_identity = (event.id, event.boundary_code);
      if previous_event != Some(event_identity) {
        unsupported(report, options, event.boundary_code, event.source)?;
        previous_event = Some(event_identity);
      }
      self.next += 1;
    }
    Ok(())
  }
}

fn collect_document_bookmark_events(
  tree: &olecfsdk::doc::DocContentTree<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Vec<DocRangeEvent>> {
  let bookmarks = tree.bookmarks()?;
  let mut events = Vec::with_capacity(bookmarks.bookmarks().len().saturating_mul(2));
  for bookmark in bookmarks.bookmarks() {
    let text = bookmark.text();
    let range = text.local_cp_range();
    let source = SourceLocation::DocBookmark {
      bookmark_index: bookmark.index(),
      part: text.document_part().part(),
      start_cp: range.start.value(),
      end_cp: range.end.value(),
    };
    if text.document_part().part() != FieldDocumentPart::Main {
      unsupported(
        report,
        options,
        ConversionCode::BookmarkStoryNotMapped,
        source,
      )?;
      continue;
    }
    let name = match String::from_utf16(bookmark.name()) {
      Ok(name) => name,
      Err(_) => {
        unsupported(
          report,
          options,
          ConversionCode::BookmarkNameCompatibilityUtf16,
          source,
        )?;
        continue;
      }
    };
    if name.chars().count() > 40 {
      unsupported(
        report,
        options,
        ConversionCode::BookmarkNameNotMapped,
        source,
      )?;
      continue;
    }
    let properties = bookmark.properties();
    let (column_first, column_last) = if properties.column {
      if let Some((first, last)) = bookmark_column_range(properties) {
        (Some(first), Some(last))
      } else {
        unsupported(
          report,
          options,
          ConversionCode::BookmarkColumnRangeNotMapped,
          source,
        )?;
        (None, None)
      }
    } else {
      (None, None)
    };
    events.push(DocRangeEvent {
      cp: range.start.value(),
      id: bookmark.index(),
      source,
      boundary_code: ConversionCode::BookmarkBoundaryNotMapped,
      marker: DocRangeMarker::BookmarkStart {
        name,
        column_first,
        column_last,
      },
    });
    events.push(DocRangeEvent {
      cp: range.end.value(),
      id: bookmark.index(),
      source,
      boundary_code: ConversionCode::BookmarkBoundaryNotMapped,
      marker: DocRangeMarker::BookmarkEnd,
    });
  }
  Ok(events)
}

fn sort_range_events(events: &mut [DocRangeEvent]) {
  events.sort_by_key(|event| {
    (
      event.cp,
      matches!(
        event.marker,
        DocRangeMarker::BookmarkEnd | DocRangeMarker::CommentEnd
      ),
      event.id,
    )
  });
}

fn bookmark_column_range(properties: &olecfsdk::doc::BookmarkStart) -> Option<(i32, i32)> {
  properties
    .column_limit
    .checked_sub(1)
    .filter(|last| *last >= properties.column_start)
    .map(|last| (i32::from(properties.column_start), i32::from(last)))
}

fn convert_paragraph<'a>(
  paragraph: DocParagraphRef<'a>,
  context: ParagraphContext,
  flow: &mut DocFlowState<'_, '_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Paragraph> {
  let source = location(paragraph.document_part().part(), paragraph.local_cp_range());
  let style_state = paragraph.style_state()?;
  let style = style_state.style();
  let outline_level = style_state.outline_level().raw();
  let piece_properties = paragraph
    .text_segments()
    .next()
    .ok_or_else(|| olecfsdk::Error::InvalidData {
      offset: u64::from(paragraph.local_cp_range().start.value()),
      message: "DOC paragraph has no text-piece coverage".into(),
    })?
    .property_modifications()?;
  let (mut paragraph_properties, has_unmapped) = convert_direct_paragraph_formatting(
    paragraph
      .source()
      .properties
      .as_deref()
      .map(|properties| &properties.properties),
    piece_properties,
  );
  if has_unmapped {
    unsupported(
      report,
      options,
      ConversionCode::ParagraphFormattingNotMapped,
      source,
    )?;
  }
  paragraph_properties.paragraph_style_id = Some(ParagraphStyleId {
    val: style_id(style.style_index()),
  });
  paragraph_properties.outline_level = (outline_level < 9).then_some(OutlineLevel {
    val: i32::from(outline_level),
  });

  let DocFlowState {
    fields: document_fields,
    ranges: document_ranges,
    note_references,
    comment_references,
  } = flow;
  let fields = document_fields.for_paragraph(paragraph.local_cp_range());
  let paragraph_range = paragraph.local_cp_range();
  for field in fields {
    if paragraph_range.contains(DocCp::new(field.begin)) {
      report.record(Disposition::Mapped);
    }
  }
  let mut paragraph_choice = Vec::new();
  for segment in paragraph.formatted_text_segments() {
    let segment = segment?;
    let text = segment.text();
    let segment_source = location(paragraph.document_part().part(), text.local_cp_range());
    let text_value = text.value()?;
    let consumes_special_content_properties = matches!(
        text_value,
        DocTextPieceValueRef::String { value, .. }
            if value.contains('\u{0001}') || value.contains('\u{0014}')
    );
    let run_formatting = convert_direct_run_formatting(
      segment.character_run().source().properties.as_deref(),
      text.property_modifications()?,
      consumes_special_content_properties,
    );
    if run_formatting.has_unmapped {
      unsupported(
        report,
        options,
        ConversionCode::CharacterFormattingNotMapped,
        segment_source,
      )?;
    }
    match text_value {
      DocTextPieceValueRef::String { value, .. } => {
        convert_ranged_text(
          value,
          text.local_cp_range(),
          text.local_cp_range().end != paragraph.local_cp_range().end,
          run_formatting,
          DocTextConversion {
            source: segment_source,
            context,
            options,
            report,
            document_part: paragraph.document_part(),
            start_cp: text.local_cp_range().start,
            fields,
            note_references,
            comment_references,
            media,
          },
          document_ranges,
          &mut paragraph_choice,
        )?;
        report.record(Disposition::Mapped);
      }
      DocTextPieceValueRef::CompatibilityUtf16(_) => {
        unsupported(report, options, ConversionCode::CompatibilityUtf16, source)?
      }
    }
  }
  report.record(Disposition::Mapped);
  Ok(Paragraph {
    paragraph_properties: Some(Box::new(paragraph_properties)),
    paragraph_choice,
    ..Default::default()
  })
}

fn convert_styles(
  source: &DocFile,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Styles>> {
  let Some(source_styles) = source.table.styles.as_ref() else {
    return Ok(None);
  };
  let definitions = &source_styles.value.styles;
  let mut styles = Vec::with_capacity(definitions.len());
  for (style_index, source_style) in definitions.iter().enumerate() {
    let Some(source_style) = source_style.definition.as_ref() else {
      continue;
    };
    let style_index = u16::try_from(style_index)
      .map_err(|_| olecfsdk::Error::Limit("STSH style index exceeds u16".into()))?;
    let source_location = SourceLocation::DocStyle { style_index };
    let Some(style_type) = convert_style_kind(source_style.base.style_kind) else {
      unsupported(
        report,
        options,
        ConversionCode::StyleKindNotMapped,
        source_location,
      )?;
      continue;
    };
    let formatting = convert_style_formatting(&source_style.formatting);
    if formatting.has_unmapped {
      unsupported(
        report,
        options,
        ConversionCode::StyleFormattingNotMapped,
        source_location,
      )?;
    }

    let id = style_id(style_index);
    let name = match String::from_utf16(&source_style.name.characters) {
      Ok(name) => name,
      Err(_) => {
        unsupported(
          report,
          options,
          ConversionCode::StyleNameCompatibilityUtf16,
          source_location,
        )?;
        id.clone()
      }
    };
    let flags = source_style.base.general_flags;
    let post_2000 = source_style.post_2000;
    styles.push(Style {
      r#type: Some(style_type),
      style_id: Some(id),
      custom_style: (source_style.base.invariant_style_id == 0x0fff)
        .then_some(ooxmlsdk::simple_type::OnOffValue::True),
      style_name: Some(StyleName { val: name }),
      based_on: style_reference(definitions, source_style.base.base_style_index)
        .map(|val| BasedOn { val }),
      next_paragraph_style: style_reference(definitions, source_style.base.next_style_index)
        .map(|val| NextParagraphStyle { val }),
      linked_style: post_2000
        .and_then(|value| style_reference(definitions, value.linked_style_index))
        .map(|val| LinkedStyle { val }),
      auto_redefine: flags
        .contains(StyleGeneralFlags::AUTO_REDEFINE)
        .then(AutoRedefine::default),
      style_hidden: flags
        .contains(StyleGeneralFlags::HIDDEN)
        .then(StyleHidden::default),
      ui_priority: post_2000
        .filter(|value| value.priority <= 99)
        .map(|value| UiPriority {
          val: i32::from(value.priority),
        }),
      semi_hidden: flags
        .contains(StyleGeneralFlags::SEMI_HIDDEN)
        .then(SemiHidden::default),
      unhide_when_used: flags
        .contains(StyleGeneralFlags::UNHIDE_WHEN_USED)
        .then(UnhideWhenUsed::default),
      primary_style: flags
        .contains(StyleGeneralFlags::QUICK_FORMAT)
        .then(PrimaryStyle::default),
      locked: flags
        .contains(StyleGeneralFlags::LOCKED)
        .then(Locked::default),
      personal: flags
        .contains(StyleGeneralFlags::PERSONAL)
        .then(Personal::default),
      personal_compose: flags
        .contains(StyleGeneralFlags::PERSONAL_COMPOSE)
        .then(PersonalCompose::default),
      personal_reply: flags
        .contains(StyleGeneralFlags::PERSONAL_REPLY)
        .then(PersonalReply::default),
      style_paragraph_properties: formatting.paragraph.map(Box::new),
      style_run_properties: formatting.run.map(Box::new),
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  Ok(Some(Styles {
    xmlns: vec![XmlNamespace::known(XmlKnownNamespace::W)],
    style: styles,
    ..Default::default()
  }))
}

fn style_reference(
  styles: &[olecfsdk::doc::LengthPrefixedStyle],
  style_index: u16,
) -> Option<String> {
  styles
    .get(usize::from(style_index))
    .and_then(|style| style.definition.as_ref())
    .map(|_| style_id(style_index))
}

fn style_id(style_index: u16) -> String {
  format!("Style{style_index}")
}

const fn convert_style_kind(kind: StyleKind) -> Option<StyleValues> {
  match kind {
    StyleKind::Paragraph => Some(StyleValues::Paragraph),
    StyleKind::Character => Some(StyleValues::Character),
    StyleKind::Table => Some(StyleValues::Table),
    StyleKind::Numbering => Some(StyleValues::Numbering),
    StyleKind::Compatibility(_) => None,
  }
}

struct ConvertedStyleFormatting {
  paragraph: Option<StyleParagraphProperties>,
  run: Option<StyleRunProperties>,
  has_unmapped: bool,
}

fn convert_style_formatting(formatting: &StyleFormatting) -> ConvertedStyleFormatting {
  let (paragraph, character, intrinsically_unmapped) = match formatting {
    StyleFormatting::Paragraph {
      paragraph,
      character,
    } => (
      Some(&paragraph.properties),
      Some(&character.properties),
      false,
    ),
    StyleFormatting::Character { character } => (None, Some(&character.properties), false),
    StyleFormatting::RevisionParagraph {
      paragraph,
      character,
      revision: _,
      original_paragraph: _,
      original_character: _,
    } => (
      Some(&paragraph.properties),
      Some(&character.properties),
      true,
    ),
    StyleFormatting::RevisionCharacter {
      character,
      revision: _,
      original_character: _,
    } => (None, Some(&character.properties), true),
    StyleFormatting::Table {
      table,
      paragraph,
      character,
    } => (
      Some(&paragraph.properties),
      Some(&character.properties),
      !table.properties.properties.is_empty(),
    ),
    StyleFormatting::Numbering { paragraph } => (Some(&paragraph.properties), None, false),
  };
  let (paragraph, paragraph_unmapped) = paragraph.map_or((None, false), convert_style_paragraph);
  let (run, run_unmapped) = character.map_or((None, false), convert_style_run);
  ConvertedStyleFormatting {
    paragraph,
    run,
    has_unmapped: intrinsically_unmapped || paragraph_unmapped || run_unmapped,
  }
}

#[derive(Default)]
struct ParagraphStyleMapping {
  properties: StyleParagraphProperties,
  mapped: bool,
  logical_justification: bool,
  logical_left_indent: bool,
  logical_right_indent: bool,
  logical_first_line_indent: bool,
}

fn convert_style_paragraph(source: &GrpPrl) -> (Option<StyleParagraphProperties>, bool) {
  let mut target = ParagraphStyleMapping::default();
  let mut has_unmapped = false;
  for property in &source.properties {
    if convert_style_paragraph_property(&mut target, property) {
      target.mapped = true;
    } else {
      has_unmapped = true;
    }
  }
  (target.mapped.then_some(target.properties), has_unmapped)
}

fn convert_style_paragraph_property(target: &mut ParagraphStyleMapping, property: &Prl) -> bool {
  match property.sprm.kind() {
    SprmKind::Known(KnownSprm::PFKeep) => set_on_off!(
      target.properties.keep_lines,
      KeepLines,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFKeepFollow) => set_on_off!(
      target.properties.keep_next,
      KeepNext,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFPageBreakBefore) => set_on_off!(
      target.properties.page_break_before,
      PageBreakBefore,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFNoLineNumb) => set_on_off!(
      target.properties.suppress_line_numbers,
      SuppressLineNumbers,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFNoAutoHyph) => set_on_off!(
      target.properties.suppress_auto_hyphens,
      SuppressAutoHyphens,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFWidowControl) => set_on_off!(
      target.properties.widow_control,
      WidowControl,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFKinsoku) => {
      set_on_off!(target.properties.kinsoku, Kinsoku, bool8(&property.operand))
    }
    SprmKind::Known(KnownSprm::PFOverflowPunct) => set_on_off!(
      target.properties.overflow_punctuation,
      OverflowPunctuation,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFTopLinePunct) => set_on_off!(
      target.properties.top_line_punctuation,
      TopLinePunctuation,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFAutoSpaceDE) => set_on_off!(
      target.properties.auto_space_de,
      AutoSpaceDe,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFAutoSpaceDN) => set_on_off!(
      target.properties.auto_space_dn,
      AutoSpaceDn,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFBiDi) => {
      set_on_off!(target.properties.bi_di, BiDi, bool8(&property.operand))
    }
    SprmKind::Known(KnownSprm::PFUsePgsuSettings) => set_on_off!(
      target.properties.snap_to_grid,
      SnapToGrid,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFAdjustRight) => set_on_off!(
      target.properties.adjust_right_indent,
      AdjustRightIndent,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFDyaBeforeAuto) => {
      let Some(value) = bool8(&property.operand) else {
        return false;
      };
      target
        .properties
        .spacing_between_lines
        .get_or_insert_with(SpacingBetweenLines::default)
        .before_auto_spacing = Some(OnOffValue::from_bool(value));
      true
    }
    SprmKind::Known(KnownSprm::PFDyaAfterAuto) => {
      let Some(value) = bool8(&property.operand) else {
        return false;
      };
      target
        .properties
        .spacing_between_lines
        .get_or_insert_with(SpacingBetweenLines::default)
        .after_auto_spacing = Some(OnOffValue::from_bool(value));
      true
    }
    SprmKind::Known(KnownSprm::PFNoAllowOverlap) => set_on_off!(
      target.properties.suppress_overlap,
      SuppressOverlap,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFContextualSpacing) => set_on_off!(
      target.properties.contextual_spacing,
      ContextualSpacing,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PFMirrorIndents) => set_on_off!(
      target.properties.mirror_indents,
      MirrorIndents,
      bool8(&property.operand)
    ),
    SprmKind::Known(KnownSprm::PJc) => {
      let Some(value) = byte(&property.operand).and_then(logical_justification) else {
        return false;
      };
      target.properties.justification = Some(Justification { val: value });
      target.logical_justification = true;
      true
    }
    SprmKind::Known(KnownSprm::PJc80) => {
      let Some(value) = byte(&property.operand).and_then(physical_justification) else {
        return false;
      };
      if !target.logical_justification {
        target.properties.justification = Some(Justification { val: value });
      }
      true
    }
    SprmKind::Known(KnownSprm::POutLvl) => {
      let Some(value @ 0..=9) = byte(&property.operand) else {
        return false;
      };
      target.properties.outline_level = Some(OutlineLevel {
        val: i32::from(value),
      });
      true
    }
    SprmKind::Known(KnownSprm::PDyaBefore) => {
      let Some(value) = word_u16(&property.operand).filter(|value| *value <= 0x7bc0) else {
        return false;
      };
      target
        .properties
        .spacing_between_lines
        .get_or_insert_with(SpacingBetweenLines::default)
        .before = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      true
    }
    SprmKind::Known(KnownSprm::PDyaAfter) => {
      let Some(value) = word_u16(&property.operand).filter(|value| *value <= 0x7bc0) else {
        return false;
      };
      target
        .properties
        .spacing_between_lines
        .get_or_insert_with(SpacingBetweenLines::default)
        .after = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      true
    }
    SprmKind::Known(KnownSprm::PDyaLine) => {
      let Some((line, line_rule)) = line_spacing(&property.operand) else {
        return false;
      };
      let spacing = target
        .properties
        .spacing_between_lines
        .get_or_insert_with(SpacingBetweenLines::default);
      spacing.line = Some(SignedTwipsMeasureValue::Twips(i64::from(line)));
      spacing.line_rule = Some(line_rule);
      true
    }
    SprmKind::Known(KnownSprm::PDxaLeft) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      let indentation = target
        .properties
        .indentation
        .get_or_insert_with(Indentation::default);
      indentation.left = None;
      indentation.start = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      target.logical_left_indent = true;
      true
    }
    SprmKind::Known(KnownSprm::PDxaLeft80) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      if !target.logical_left_indent {
        target
          .properties
          .indentation
          .get_or_insert_with(Indentation::default)
          .left = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      }
      true
    }
    SprmKind::Known(KnownSprm::PDxaRight) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      let indentation = target
        .properties
        .indentation
        .get_or_insert_with(Indentation::default);
      indentation.right = None;
      indentation.end = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      target.logical_right_indent = true;
      true
    }
    SprmKind::Known(KnownSprm::PDxaRight80) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      if !target.logical_right_indent {
        target
          .properties
          .indentation
          .get_or_insert_with(Indentation::default)
          .right = Some(SignedTwipsMeasureValue::Twips(i64::from(value)));
      }
      true
    }
    SprmKind::Known(KnownSprm::PDxaLeft1) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      set_first_line_indent(&mut target.properties, value);
      target.logical_first_line_indent = true;
      true
    }
    SprmKind::Known(KnownSprm::PDxaLeft180) => {
      let Some(value) = word_i16(&property.operand) else {
        return false;
      };
      if !target.logical_first_line_indent {
        set_first_line_indent(&mut target.properties, value);
      }
      true
    }
    _ => false,
  }
}

fn set_first_line_indent(target: &mut StyleParagraphProperties, value: i16) {
  let indentation = target.indentation.get_or_insert_with(Indentation::default);
  if value < 0 {
    indentation.first_line = None;
    indentation.hanging = Some(SignedTwipsMeasureValue::Twips(i64::from(
      value.unsigned_abs(),
    )));
  } else {
    indentation.hanging = None;
    indentation.first_line = Some(TwipsMeasureValue::Twips(u64::from(value.unsigned_abs())));
  }
}

fn convert_direct_paragraph_formatting(
  papx: Option<&GrpPrl>,
  piece: PrmPropertiesRef<'_>,
) -> (ParagraphProperties, bool) {
  let mut target = ParagraphStyleMapping::default();
  let mut has_unmapped = false;
  if let Some(papx) = papx {
    apply_direct_paragraph_properties(papx, &mut target, &mut has_unmapped);
  }
  match piece {
    PrmPropertiesRef::Empty => {}
    PrmPropertiesRef::Simple { sprm, value } => {
      let sprm_value = Sprm::from_opcode(sprm.opcode());
      if sprm_value.group == SprmGroup::Paragraph {
        match inline_property(sprm, value) {
          Some(property) => {
            if !convert_style_paragraph_property(&mut target, &property) {
              has_unmapped = true;
            }
          }
          None => has_unmapped = true,
        }
      }
    }
    PrmPropertiesRef::Complex(properties) => {
      apply_direct_paragraph_properties(properties, &mut target, &mut has_unmapped);
    }
  }
  (
    style_paragraph_to_paragraph(target.properties),
    has_unmapped,
  )
}

fn apply_direct_paragraph_properties(
  source: &GrpPrl,
  target: &mut ParagraphStyleMapping,
  has_unmapped: &mut bool,
) {
  for property in &source.properties {
    if property.sprm.group == SprmGroup::Paragraph
      && !convert_style_paragraph_property(target, property)
    {
      *has_unmapped = true;
    }
  }
}

fn style_paragraph_to_paragraph(source: StyleParagraphProperties) -> ParagraphProperties {
  ParagraphProperties {
    keep_next: source.keep_next,
    keep_lines: source.keep_lines,
    page_break_before: source.page_break_before,
    widow_control: source.widow_control,
    suppress_line_numbers: source.suppress_line_numbers,
    suppress_auto_hyphens: source.suppress_auto_hyphens,
    kinsoku: source.kinsoku,
    overflow_punctuation: source.overflow_punctuation,
    top_line_punctuation: source.top_line_punctuation,
    auto_space_de: source.auto_space_de,
    auto_space_dn: source.auto_space_dn,
    bi_di: source.bi_di,
    adjust_right_indent: source.adjust_right_indent,
    snap_to_grid: source.snap_to_grid,
    spacing_between_lines: source.spacing_between_lines,
    indentation: source.indentation,
    contextual_spacing: source.contextual_spacing,
    mirror_indents: source.mirror_indents,
    suppress_overlap: source.suppress_overlap,
    justification: source.justification,
    outline_level: source.outline_level,
    ..Default::default()
  }
}

fn convert_style_run(source: &GrpPrl) -> (Option<StyleRunProperties>, bool) {
  let mut target = StyleRunProperties::default();
  let mut mapped = false;
  let mut has_unmapped = false;
  for property in &source.properties {
    if convert_style_run_property(&mut target, property) {
      mapped = true;
    } else {
      has_unmapped = true;
    }
  }
  (mapped.then_some(target), has_unmapped)
}

fn convert_style_run_property(target: &mut StyleRunProperties, property: &Prl) -> bool {
  match property.sprm.kind() {
    SprmKind::Known(KnownSprm::CFBold) => {
      set_on_off!(target.bold, Bold, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFItalic) => {
      set_on_off!(target.italic, Italic, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFStrike) => {
      set_on_off!(target.strike, Strike, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFOutline) => {
      set_on_off!(target.outline, Outline, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFShadow) => {
      set_on_off!(target.shadow, Shadow, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFSmallCaps) => {
      set_on_off!(target.small_caps, SmallCaps, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFCaps) => {
      set_on_off!(target.caps, Caps, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFVanish) => {
      set_on_off!(target.vanish, Vanish, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFWebHidden) => {
      set_on_off!(target.web_hidden, WebHidden, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFDStrike) => {
      set_on_off!(target.double_strike, DoubleStrike, bool8(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFImprint) => {
      set_on_off!(target.imprint, Imprint, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFEmboss) => {
      set_on_off!(target.emboss, Emboss, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFNoProof) => {
      set_on_off!(target.no_proof, NoProof, toggle(&property.operand))
    }
    SprmKind::Known(KnownSprm::CFBiDi) => set_on_off!(
      target.right_to_left_text,
      RightToLeftText,
      toggle(&property.operand)
    ),
    SprmKind::Known(KnownSprm::CFBoldBi) => set_on_off!(
      target.bold_complex_script,
      BoldComplexScript,
      toggle(&property.operand)
    ),
    SprmKind::Known(KnownSprm::CFItalicBi) => set_on_off!(
      target.italic_complex_script,
      ItalicComplexScript,
      toggle(&property.operand)
    ),
    SprmKind::Known(KnownSprm::CHps) => {
      let Some(value) = word_u16(&property.operand).filter(|value| (2..=3276).contains(value))
      else {
        return false;
      };
      target.font_size = Some(FontSize {
        val: HpsMeasureValue::HalfPoints(u64::from(value)),
      });
      true
    }
    SprmKind::Known(KnownSprm::CHpsBi) => {
      let Some(value) = word_u16(&property.operand).filter(|value| (2..=3276).contains(value))
      else {
        return false;
      };
      target.font_size_complex_script = Some(FontSizeComplexScript {
        val: HpsMeasureValue::HalfPoints(u64::from(value)),
      });
      true
    }
    SprmKind::Known(KnownSprm::CKul) => {
      let Some(value) = byte(&property.operand).and_then(underline) else {
        return false;
      };
      target.underline = Some(Underline {
        val: Some(value),
        ..Default::default()
      });
      true
    }
    _ => false,
  }
}

#[derive(Clone, Default)]
struct ConvertedRunFormatting {
  properties: Option<RunProperties>,
  rsid_properties: Option<String>,
  rsid_text: Option<String>,
  rsid_deletion: Option<String>,
  has_unmapped: bool,
}

fn convert_direct_run_formatting(
  chpx: Option<&GrpPrl>,
  piece: PrmPropertiesRef<'_>,
  consumes_special_content_properties: bool,
) -> ConvertedRunFormatting {
  let mut target = StyleRunProperties::default();
  let mut mapped = false;
  let mut result = ConvertedRunFormatting::default();
  if let Some(chpx) = chpx {
    apply_direct_run_properties(
      chpx,
      &mut target,
      &mut mapped,
      &mut result,
      consumes_special_content_properties,
    );
  }
  match piece {
    PrmPropertiesRef::Empty => {}
    PrmPropertiesRef::Simple { sprm, value } => {
      let sprm_value = Sprm::from_opcode(sprm.opcode());
      if sprm_value.group == SprmGroup::Character {
        if consumes_special_content_properties && is_special_content_property(sprm) {
          result.properties = mapped.then(|| style_run_to_run(target));
          return result;
        }
        match inline_property(sprm, value) {
          Some(property) => {
            if convert_style_run_property(&mut target, &property) {
              mapped = true;
            } else {
              result.has_unmapped = true;
            }
          }
          None => result.has_unmapped = true,
        }
      }
    }
    PrmPropertiesRef::Complex(properties) => {
      apply_direct_run_properties(
        properties,
        &mut target,
        &mut mapped,
        &mut result,
        consumes_special_content_properties,
      );
    }
  }
  result.properties = mapped.then(|| style_run_to_run(target));
  result
}

fn inline_property(sprm: KnownSprm, value: u8) -> Option<Prl> {
  let sprm = Sprm::from_opcode(sprm.opcode());
  let operand = match sprm.operand_size {
    SprmOperandSize::Toggle => SprmOperand::Toggle(value),
    SprmOperandSize::Byte => SprmOperand::Byte(value),
    _ => return None,
  };
  Some(Prl { sprm, operand })
}

fn apply_direct_run_properties(
  source: &GrpPrl,
  target: &mut StyleRunProperties,
  mapped: &mut bool,
  result: &mut ConvertedRunFormatting,
  consumes_special_content_properties: bool,
) {
  for property in &source.properties {
    if property.sprm.group != SprmGroup::Character {
      continue;
    }
    if consumes_special_content_properties
      && matches!(
          property.sprm.kind(),
          SprmKind::Known(value) if is_special_content_property(value)
      )
    {
      continue;
    }
    if apply_run_revision_id(result, property) {
      continue;
    }
    if convert_style_run_property(target, property) {
      *mapped = true;
    } else {
      result.has_unmapped = true;
    }
  }
}

fn apply_run_revision_id(target: &mut ConvertedRunFormatting, property: &Prl) -> bool {
  let destination = match property.sprm.kind() {
    SprmKind::Known(KnownSprm::CRsidProp) => &mut target.rsid_properties,
    SprmKind::Known(KnownSprm::CRsidText) => &mut target.rsid_text,
    SprmKind::Known(KnownSprm::CRsidRMDel) => &mut target.rsid_deletion,
    _ => return false,
  };
  let Some(value) = dword_u32(&property.operand) else {
    target.has_unmapped = true;
    return true;
  };
  *destination = Some(format!("{value:08X}"));
  true
}

const fn is_special_content_property(sprm: KnownSprm) -> bool {
  matches!(
    sprm,
    KnownSprm::CPicLocation
      | KnownSprm::CFSpec
      | KnownSprm::CFData
      | KnownSprm::CFOle2
      | KnownSprm::CFObj
  )
}

fn style_run_to_run(source: StyleRunProperties) -> RunProperties {
  let mut choices = Vec::new();
  if let Some(value) = source.bold {
    choices.push(RunPropertiesChoice::Bold(value));
  }
  if let Some(value) = source.bold_complex_script {
    choices.push(RunPropertiesChoice::BoldComplexScript(value));
  }
  if let Some(value) = source.italic {
    choices.push(RunPropertiesChoice::Italic(value));
  }
  if let Some(value) = source.italic_complex_script {
    choices.push(RunPropertiesChoice::ItalicComplexScript(value));
  }
  if let Some(value) = source.caps {
    choices.push(RunPropertiesChoice::Caps(value));
  }
  if let Some(value) = source.small_caps {
    choices.push(RunPropertiesChoice::SmallCaps(value));
  }
  if let Some(value) = source.strike {
    choices.push(RunPropertiesChoice::Strike(value));
  }
  if let Some(value) = source.double_strike {
    choices.push(RunPropertiesChoice::DoubleStrike(value));
  }
  if let Some(value) = source.outline {
    choices.push(RunPropertiesChoice::Outline(value));
  }
  if let Some(value) = source.shadow {
    choices.push(RunPropertiesChoice::Shadow(value));
  }
  if let Some(value) = source.emboss {
    choices.push(RunPropertiesChoice::Emboss(value));
  }
  if let Some(value) = source.imprint {
    choices.push(RunPropertiesChoice::Imprint(value));
  }
  if let Some(value) = source.no_proof {
    choices.push(RunPropertiesChoice::NoProof(value));
  }
  if let Some(value) = source.vanish {
    choices.push(RunPropertiesChoice::Vanish(value));
  }
  if let Some(value) = source.web_hidden {
    choices.push(RunPropertiesChoice::WebHidden(value));
  }
  if let Some(value) = source.font_size {
    choices.push(RunPropertiesChoice::FontSize(value));
  }
  if let Some(value) = source.font_size_complex_script {
    choices.push(RunPropertiesChoice::FontSizeComplexScript(value));
  }
  if let Some(value) = source.underline {
    choices.push(RunPropertiesChoice::Underline(Box::new(value)));
  }
  if let Some(value) = source.right_to_left_text {
    choices.push(RunPropertiesChoice::RightToLeftText(value));
  }
  RunProperties {
    run_properties_choice: choices,
    ..Default::default()
  }
}

const fn bool8(operand: &SprmOperand) -> Option<bool> {
  match operand {
    SprmOperand::Byte(0) => Some(false),
    SprmOperand::Byte(1) => Some(true),
    _ => None,
  }
}

const fn toggle(operand: &SprmOperand) -> Option<bool> {
  match operand {
    SprmOperand::Toggle(0) => Some(false),
    SprmOperand::Toggle(1) => Some(true),
    _ => None,
  }
}

const fn byte(operand: &SprmOperand) -> Option<u8> {
  match operand {
    SprmOperand::Byte(value) => Some(*value),
    _ => None,
  }
}

fn word_u16(operand: &SprmOperand) -> Option<u16> {
  match operand {
    SprmOperand::Word(value) | SprmOperand::Word4(value) | SprmOperand::Word5(value) => {
      Some(u16::from_le_bytes(*value))
    }
    _ => None,
  }
}

fn word_i16(operand: &SprmOperand) -> Option<i16> {
  word_u16(operand).map(|value| i16::from_le_bytes(value.to_le_bytes()))
}

fn dword_u32(operand: &SprmOperand) -> Option<u32> {
  match operand {
    SprmOperand::Dword(value) => Some(u32::from_le_bytes(*value)),
    _ => None,
  }
}

fn line_spacing(operand: &SprmOperand) -> Option<(u16, LineSpacingRuleValues)> {
  let SprmOperand::Dword(value) = operand else {
    return None;
  };
  let line = i16::from_le_bytes([value[0], value[1]]);
  let multiple = u16::from_le_bytes([value[2], value[3]]);
  match (line, multiple) {
    (-31_680..=-1, 0) => Some((line.unsigned_abs(), LineSpacingRuleValues::Exact)),
    (0..=31_680, 0) => Some((line.unsigned_abs(), LineSpacingRuleValues::AtLeast)),
    (0..=31_680, 1) => Some((line.unsigned_abs(), LineSpacingRuleValues::Auto)),
    _ => None,
  }
}

const fn logical_justification(value: u8) -> Option<JustificationValues> {
  match value {
    0 => Some(JustificationValues::Left),
    1 => Some(JustificationValues::Center),
    2 => Some(JustificationValues::Right),
    3 => Some(JustificationValues::Both),
    4 => Some(JustificationValues::Distribute),
    5 => Some(JustificationValues::MediumKashida),
    7 => Some(JustificationValues::HighKashida),
    8 => Some(JustificationValues::LowKashida),
    9 => Some(JustificationValues::ThaiDistribute),
    _ => None,
  }
}

const fn physical_justification(value: u8) -> Option<JustificationValues> {
  match value {
    0 => Some(JustificationValues::Left),
    1 => Some(JustificationValues::Center),
    2 => Some(JustificationValues::Right),
    3 => Some(JustificationValues::Both),
    4 => Some(JustificationValues::MediumKashida),
    5 => Some(JustificationValues::HighKashida),
    _ => None,
  }
}

const fn underline(value: u8) -> Option<UnderlineValues> {
  match value {
    0x00 => Some(UnderlineValues::None),
    0x01 => Some(UnderlineValues::Single),
    0x02 => Some(UnderlineValues::Words),
    0x03 => Some(UnderlineValues::Double),
    0x04 => Some(UnderlineValues::Dotted),
    0x06 => Some(UnderlineValues::Thick),
    0x07 => Some(UnderlineValues::Dash),
    0x09 => Some(UnderlineValues::DotDash),
    0x0a => Some(UnderlineValues::DotDotDash),
    0x0b => Some(UnderlineValues::Wave),
    0x14 => Some(UnderlineValues::DottedHeavy),
    0x17 => Some(UnderlineValues::DashedHeavy),
    0x19 => Some(UnderlineValues::DashDotHeavy),
    0x1a => Some(UnderlineValues::DashDotDotHeavy),
    0x1b => Some(UnderlineValues::WavyHeavy),
    0x27 => Some(UnderlineValues::DashLong),
    0x2b => Some(UnderlineValues::WavyDouble),
    0x37 => Some(UnderlineValues::DashLongHeavy),
    _ => None,
  }
}

fn convert_table<'a>(
  table: &DocTableRef<'a>,
  tables: &DocTables<'a>,
  cell_context: ParagraphContext,
  flow: &mut DocFlowState<'_, '_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Table> {
  unsupported(
    report,
    options,
    ConversionCode::TableFormattingNotMapped,
    location(table.document_part().part(), table.local_cp_range()),
  )?;
  let mut table_choice2 = Vec::with_capacity(table.rows().len());
  for row in table.rows() {
    let cells = row.cells()?;
    let mut table_row_choice = Vec::with_capacity(cells.cells().len());
    for cell in cells.cells() {
      table_row_choice.push(TableRowChoice::TableCell(Box::new(convert_cell(
        *cell,
        tables,
        cell_context,
        flow,
        options,
        report,
        media,
      )?)));
    }
    table_choice2.push(TableChoice2::TableRow(Box::new(TableRow {
      table_row_choice,
      ..Default::default()
    })));
    report.record(Disposition::Mapped);
  }
  report.record(Disposition::Mapped);
  Ok(Table {
    table_choice2,
    ..Default::default()
  })
}

fn convert_cell<'a>(
  cell: DocTableCellRef<'a>,
  tables: &DocTables<'a>,
  context: ParagraphContext,
  flow: &mut DocFlowState<'_, '_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<TableCell> {
  let mut table_cell_choice = Vec::new();
  for block in cell.blocks_with_tables(tables)?.blocks() {
    match block {
      DocBlockRef::Paragraph(paragraph) => {
        table_cell_choice.push(TableCellChoice::Paragraph(Box::new(convert_paragraph(
          *paragraph, context, flow, options, report, media,
        )?)));
      }
      DocBlockRef::Table(table) => table_cell_choice.push(TableCellChoice::Table(Box::new(
        convert_table(table, tables, context, flow, options, report, media)?,
      ))),
    }
  }
  if table_cell_choice.is_empty() {
    table_cell_choice.push(TableCellChoice::Paragraph(Box::default()));
  }
  report.record(Disposition::Mapped);
  Ok(TableCell {
    table_cell_choice,
    ..Default::default()
  })
}

struct DocTextConversion<'a, 'state> {
  source: SourceLocation,
  context: ParagraphContext,
  options: ConversionOptions,
  report: &'state mut ConversionReport,
  document_part: DocDocumentPartRef<'a>,
  start_cp: DocCp,
  fields: &'state [DocFieldSpan],
  note_references: &'state [DocNoteReference],
  comment_references: &'state [DocCommentReference],
  media: &'state mut DocMediaState<'a>,
}

fn convert_ranged_text<'a>(
  value: &str,
  range: DocCpRange,
  include_end_bookmarks: bool,
  formatting: ConvertedRunFormatting,
  conversion: DocTextConversion<'a, '_>,
  ranges: &mut DocRangeCursor,
  paragraph_choice: &mut Vec<ParagraphChoice>,
) -> Result<()> {
  let DocTextConversion {
    source,
    context,
    options,
    report,
    document_part,
    fields,
    note_references,
    comment_references,
    media,
    ..
  } = conversion;
  if ranges.next_position().is_none_or(|position| {
    position > range.end.value() || (position == range.end.value() && !include_end_bookmarks)
  }) {
    push_doc_text_run(
      value,
      range.start,
      formatting,
      DocTextConversion {
        source,
        context,
        options,
        report,
        document_part,
        start_cp: range.start,
        fields,
        note_references,
        comment_references,
        media,
      },
      paragraph_choice,
    )?;
    return Ok(());
  }

  let mut formatting = Some(formatting);
  let mut text_start = 0usize;
  let mut text_start_cp = range.start.value();
  let mut cp = range.start.value();
  for (index, character) in value.char_indices() {
    if ranges
      .next_position()
      .is_some_and(|position| position <= cp)
    {
      push_range_text_chunk(
        &value[text_start..index],
        text_start_cp,
        index < value.len(),
        &mut formatting,
        DocTextConversion {
          source,
          context,
          options,
          report: &mut *report,
          document_part,
          start_cp: DocCp::new(text_start_cp),
          fields,
          note_references,
          comment_references,
          media: &mut *media,
        },
        paragraph_choice,
      )?;
      while ranges
        .next_position()
        .is_some_and(|position| position <= cp)
      {
        if ranges.next_position() != Some(cp) {
          unsupported(
            report,
            options,
            ranges
              .events
              .get(ranges.next)
              .expect("a pending range event exists")
              .boundary_code,
            ranges
              .next_source()
              .expect("a pending bookmark position has a source"),
          )?;
        }
        ranges.emit_next(paragraph_choice, report);
      }
      text_start = index;
      text_start_cp = cp;
    }
    cp = cp
      .checked_add(
        u32::try_from(character.len_utf16()).expect("a char uses at most two UTF-16 units"),
      )
      .ok_or_else(|| olecfsdk::Error::Limit("DOC text CP overflow".into()))?;
  }
  if ranges
    .next_position()
    .is_some_and(|position| position < cp || (include_end_bookmarks && position == cp))
  {
    push_range_text_chunk(
      &value[text_start..],
      text_start_cp,
      false,
      &mut formatting,
      DocTextConversion {
        source,
        context,
        options,
        report: &mut *report,
        document_part,
        start_cp: DocCp::new(text_start_cp),
        fields,
        note_references,
        comment_references,
        media: &mut *media,
      },
      paragraph_choice,
    )?;
    while ranges
      .next_position()
      .is_some_and(|position| position < cp || (include_end_bookmarks && position == cp))
    {
      if ranges.next_position() != Some(cp) {
        unsupported(
          report,
          options,
          ranges
            .events
            .get(ranges.next)
            .expect("a pending range event exists")
            .boundary_code,
          ranges
            .next_source()
            .expect("a pending bookmark position has a source"),
        )?;
      }
      ranges.emit_next(paragraph_choice, report);
    }
    text_start = value.len();
    text_start_cp = cp;
  }
  if text_start < value.len() {
    push_doc_text_run(
      &value[text_start..],
      DocCp::new(text_start_cp),
      formatting.expect("run formatting is retained for the final text chunk"),
      DocTextConversion {
        source,
        context,
        options,
        report,
        document_part,
        start_cp: DocCp::new(text_start_cp),
        fields,
        note_references,
        comment_references,
        media,
      },
      paragraph_choice,
    )?;
  }
  Ok(())
}

fn push_range_text_chunk(
  value: &str,
  start_cp: u32,
  has_later_text: bool,
  formatting: &mut Option<ConvertedRunFormatting>,
  conversion: DocTextConversion<'_, '_>,
  paragraph_choice: &mut Vec<ParagraphChoice>,
) -> Result<()> {
  if value.is_empty() {
    return Ok(());
  }
  let run_formatting = if has_later_text {
    formatting
      .as_ref()
      .expect("run formatting is available for split text")
      .clone()
  } else {
    formatting
      .take()
      .expect("run formatting is available for the final text chunk")
  };
  push_doc_text_run(
    value,
    DocCp::new(start_cp),
    run_formatting,
    conversion,
    paragraph_choice,
  )
}

fn push_doc_text_run(
  value: &str,
  start_cp: DocCp,
  formatting: ConvertedRunFormatting,
  mut conversion: DocTextConversion<'_, '_>,
  paragraph_choice: &mut Vec<ParagraphChoice>,
) -> Result<()> {
  conversion.start_cp = start_cp;
  let run_choice = convert_text(value, conversion)?;
  if !run_choice.is_empty() {
    paragraph_choice.push(ParagraphChoice::WRun(Box::new(Run {
      rsid_run_properties: formatting.rsid_properties,
      rsid_run_deletion: formatting.rsid_deletion,
      rsid_run_addition: formatting.rsid_text,
      run_properties: formatting.properties.map(Box::new),
      run_choice,
    })));
  }
  Ok(())
}

#[derive(Clone, Copy)]
struct DocFieldSpan {
  begin: u32,
  separator: Option<u32>,
  end: u32,
  locked: bool,
  dirty: bool,
}

struct DocFieldCursor {
  fields: Vec<DocFieldSpan>,
  active: Vec<DocFieldSpan>,
  next: usize,
  previous_paragraph_start: Option<u32>,
}

impl DocFieldCursor {
  fn new(fields: Vec<DocFieldSpan>) -> Self {
    Self {
      fields,
      active: Vec::new(),
      next: 0,
      previous_paragraph_start: None,
    }
  }

  fn for_paragraph(&mut self, paragraph: DocCpRange) -> &[DocFieldSpan] {
    let start = paragraph.start.value();
    let end = paragraph.end.value();
    if self
      .previous_paragraph_start
      .is_some_and(|previous| start < previous)
    {
      self.active.clear();
      self.next = 0;
    }
    self.previous_paragraph_start = Some(start);
    self.active.retain(|field| field.end >= start);
    while self
      .fields
      .get(self.next)
      .is_some_and(|field| field.begin < end)
    {
      let field = self.fields[self.next];
      if field.end >= start {
        self.active.push(field);
      }
      self.next += 1;
    }
    &self.active
  }
}

fn collect_document_field_spans(document_part: DocDocumentPartRef<'_>) -> Vec<DocFieldSpan> {
  let mut fields = Vec::new();
  for field in document_part.fields() {
    collect_field_span(field, &mut fields);
  }
  fields.sort_unstable_by_key(|field| field.begin);
  fields
}

fn collect_field_span(field: DocFieldRef<'_>, fields: &mut Vec<DocFieldSpan>) {
  let source = field.source();
  fields.push(DocFieldSpan {
    begin: source.begin.position,
    separator: source.separator.map(|separator| separator.position),
    end: source.end.position,
    locked: source.end.flags.contains(FieldEndFlags::LOCKED),
    dirty: source.end.flags.contains(FieldEndFlags::RESULTS_DIRTY),
  });
  for child in field.instruction_fields() {
    collect_field_span(child, fields);
  }
  for child in field.result_fields() {
    collect_field_span(child, fields);
  }
}

fn convert_text(value: &str, conversion: DocTextConversion<'_, '_>) -> Result<Vec<RunChoice>> {
  let DocTextConversion {
    source,
    context,
    options,
    report,
    document_part,
    start_cp,
    fields,
    note_references,
    comment_references,
    media,
  } = conversion;
  let mut choices = Vec::new();
  let mut text_start = 0;
  let mut text_start_cp = start_cp.value();
  let mut cp = start_cp.value();
  for (index, character) in value.char_indices() {
    let character_cp = cp;
    cp = cp
      .checked_add(
        u32::try_from(character.len_utf16()).expect("a char uses at most two UTF-16 units"),
      )
      .ok_or_else(|| olecfsdk::Error::Limit("DOC text CP overflow".into()))?;
    let field_marker = convert_field_marker(fields, character_cp, character);
    let mapped = match character {
      _ if field_marker.is_some() => field_marker.map(Some),
      '\r' => Some(None),
      '\u{0007}' if context.is_table_cell() => Some(None),
      '\t' => Some(Some(RunChoice::TabChar)),
      '\u{000b}' => Some(Some(RunChoice::Break(Default::default()))),
      '\u{0002}' => {
        let mapped = convert_note_reference(note_references, character_cp, context);
        if mapped.is_some() {
          report.record(Disposition::Mapped);
          Some(mapped)
        } else {
          unsupported(
            report,
            options,
            ConversionCode::ControlCharacterNotMapped,
            source,
          )?;
          Some(None)
        }
      }
      '\u{0005}' => {
        let mapped = convert_comment_reference(comment_references, character_cp, context);
        if mapped.is_some() {
          report.record(Disposition::Mapped);
          Some(mapped)
        } else {
          unsupported(
            report,
            options,
            ConversionCode::ControlCharacterNotMapped,
            source,
          )?;
          Some(None)
        }
      }
      '\u{0008}'
        if matches!(
          context,
          ParagraphContext::Body | ParagraphContext::TableCell
        ) =>
      {
        Some(convert_floating_shape(
          character_cp,
          source,
          options,
          report,
          media,
        )?)
      }
      '\u{0001}' | '\u{0014}' => Some(convert_special_content(
        document_part,
        DocCp::new(character_cp),
        context,
        source,
        options,
        report,
        media,
      )?),
      value if value.is_control() => {
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
    push_field_aware_text(
      &mut choices,
      &value[text_start..index],
      text_start_cp,
      fields,
    );
    if let Some(mapped) = mapped {
      choices.push(mapped);
    }
    text_start = index + character.len_utf8();
    text_start_cp = cp;
  }
  push_field_aware_text(&mut choices, &value[text_start..], text_start_cp, fields);
  Ok(choices)
}

fn convert_note_reference(
  references: &[DocNoteReference],
  cp: u32,
  context: ParagraphContext,
) -> Option<RunChoice> {
  match context {
    ParagraphContext::Footnote | ParagraphContext::FootnoteTableCell => {
      Some(RunChoice::FootnoteReferenceMark)
    }
    ParagraphContext::Endnote | ParagraphContext::EndnoteTableCell => {
      Some(RunChoice::EndnoteReferenceMark)
    }
    ParagraphContext::Body | ParagraphContext::TableCell => {
      let reference = references
        .binary_search_by_key(&cp, |reference| reference.cp)
        .ok()
        .map(|index| references[index])?;
      match reference.kind {
        DocNoteKind::Footnote => Some(RunChoice::FootnoteReference(FootnoteReference {
          id: reference.id,
          ..Default::default()
        })),
        DocNoteKind::Endnote => Some(RunChoice::EndnoteReference(EndnoteReference {
          id: reference.id,
          ..Default::default()
        })),
      }
    }
    ParagraphContext::Comment
    | ParagraphContext::CommentTableCell
    | ParagraphContext::Textbox
    | ParagraphContext::TextboxTableCell => None,
  }
}

fn convert_comment_reference(
  references: &[DocCommentReference],
  cp: u32,
  context: ParagraphContext,
) -> Option<RunChoice> {
  if matches!(
    context,
    ParagraphContext::Comment | ParagraphContext::CommentTableCell
  ) {
    return Some(RunChoice::AnnotationReferenceMark);
  }
  let reference = references
    .binary_search_by_key(&cp, |reference| reference.cp)
    .ok()
    .map(|index| references[index])?;
  Some(RunChoice::CommentReference(CommentReference {
    id: reference.id.to_string(),
  }))
}

fn convert_field_marker(fields: &[DocFieldSpan], cp: u32, character: char) -> Option<RunChoice> {
  let (field, field_char_type) = match character {
    '\u{0013}' => (
      fields.iter().find(|field| field.begin == cp)?,
      FieldCharValues::Begin,
    ),
    '\u{0014}' => (
      fields.iter().find(|field| field.separator == Some(cp))?,
      FieldCharValues::Separate,
    ),
    '\u{0015}' => (
      fields.iter().find(|field| field.end == cp)?,
      FieldCharValues::End,
    ),
    _ => return None,
  };
  Some(RunChoice::FieldChar(Box::new(FieldChar {
    field_char_type,
    field_lock: (field_char_type == FieldCharValues::Begin && field.locked)
      .then_some(OnOffValue::from_bool(true)),
    dirty: (field_char_type == FieldCharValues::Begin && field.dirty)
      .then_some(OnOffValue::from_bool(true)),
    ..Default::default()
  })))
}

fn push_field_aware_text(
  choices: &mut Vec<RunChoice>,
  value: &str,
  cp: u32,
  fields: &[DocFieldSpan],
) {
  if value.is_empty() {
    return;
  }
  let text = text_type(value);
  if field_instruction_at(fields, cp) {
    choices.push(RunChoice::FieldCode(FieldCode(text)));
  } else {
    choices.push(RunChoice::Text(Text(text)));
  }
}

fn field_instruction_at(fields: &[DocFieldSpan], cp: u32) -> bool {
  fields
    .iter()
    .filter(|field| field.begin < cp && cp < field.end)
    .min_by_key(|field| field.end - field.begin)
    .is_some_and(|field| cp < field.separator.unwrap_or(field.end))
}

fn convert_floating_shape<'a>(
  cp: u32,
  fallback_source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Option<RunChoice>> {
  let Ok(index) = media
    .floating_shapes
    .binary_search_by_key(&cp, |shape| shape.anchor.anchor_cp().value())
  else {
    unsupported(
      report,
      options,
      ConversionCode::FloatingShapeNotMapped,
      fallback_source,
    )?;
    return Ok(None);
  };
  let shape = media.floating_shapes[index];
  let source = floating_shape_location(shape);
  let office_shape = shape
    .anchor
    .shape()
    .expect("a floating anchor is linked to an OfficeArt shape");
  if shape.has_flow_breaks {
    unsupported(
      report,
      options,
      ConversionCode::TextboxFlowNotMapped,
      source,
    )?;
  }
  let Some(geometry) = floating_shape_geometry(shape.anchor, office_shape.shape_type()) else {
    unsupported(
      report,
      options,
      ConversionCode::FloatingShapeGeometryNotMapped,
      source,
    )?;
    return Ok(None);
  };
  let shape_source = shape.anchor.source();
  let horizontal_relative = floating_horizontal_origin(shape_source, report, options, source)?;
  let vertical_relative = floating_vertical_origin(shape_source, report, options, source)?;
  let anchor_choice = floating_wrap(shape_source, office_shape, report, options, source)?;
  let textbox = if let Some(story_text) = shape.story_text {
    let story_id = u16::try_from(
      shape
        .story_index
        .expect("a linked textbox shape has a story index")
        + 1,
    )
    .map_err(|_| olecfsdk::Error::Limit("DOC textbox story index exceeds u16".into()))?;
    let chain_index = shape
      .chain_index
      .expect("a linked textbox shape has a chain index");
    if chain_index == 0 {
      let mut fields = media
        .textbox_fields
        .take()
        .ok_or_else(|| olecfsdk::Error::InvalidData {
          offset: u64::from(story_text.local_cp_range().start.value()),
          message: "DOC textbox story has no field document".into(),
        })?;
      let content =
        convert_textbox_content(story_text, &mut fields, options, report, media, source);
      media.textbox_fields = Some(fields);
      Some(wps::WordprocessingShapeChoice2::TextBoxInfo2(Box::new(
        wps::TextBoxInfo2 {
          id: Some(story_id),
          text_box_content: Some(content?),
          ..Default::default()
        },
      )))
    } else {
      Some(wps::WordprocessingShapeChoice2::LinkedTextBox(Box::new(
        wps::LinkedTextBox {
          id: story_id,
          sequence: chain_index,
          ..Default::default()
        },
      )))
    }
  } else {
    None
  };
  let primary_blip_identifier = office_shape.primary_blip_identifier()?;
  let is_picture = shape.story_text.is_none()
    && (office_shape.shape_type() == 75 || primary_blip_identifier.is_some());
  let floating_image = if is_picture {
    let Some(blip_identifier) = primary_blip_identifier else {
      unsupported(
        report,
        options,
        ConversionCode::FloatingPictureNotMapped,
        source,
      )?;
      return Ok(None);
    };
    let document = media
      .source
      .expect("DOC conversion media state retains its source");
    match document.office_art_image_link(blip_identifier)? {
      Some(DocOfficeArtImageLink::Resolved(image)) => Some(image),
      Some(DocOfficeArtImageLink::Delayed { .. })
      | Some(DocOfficeArtImageLink::Unsupported)
      | None => {
        unsupported(
          report,
          options,
          ConversionCode::FloatingPictureNotMapped,
          source,
        )?;
        return Ok(None);
      }
    }
  } else {
    None
  };
  let mut formatting_loss = false;
  let shape_fill = if is_picture {
    None
  } else {
    floating_shape_fill(office_shape, &mut formatting_loss)
  };
  let picture_fill = if is_picture {
    floating_picture_fill(office_shape, &mut formatting_loss)
  } else {
    None
  };
  let picture_crop = if is_picture {
    floating_picture_crop(office_shape, &mut formatting_loss)
  } else {
    None
  };
  let shape_outline = floating_shape_outline(office_shape, &mut formatting_loss);
  let wrap_distances = office_shape.wrap_distances();
  let distance_from_left = floating_distance(wrap_distances.left(), &mut formatting_loss);
  let distance_from_top = floating_distance(wrap_distances.top(), &mut formatting_loss);
  let distance_from_right = floating_distance(wrap_distances.right(), &mut formatting_loss);
  let distance_from_bottom = floating_distance(wrap_distances.bottom(), &mut formatting_loss);
  if formatting_loss {
    unsupported(
      report,
      options,
      ConversionCode::FloatingShapeFormattingNotMapped,
      source,
    )?;
  }
  let preset = match is_picture
    .then_some(a::ShapeTypeValues::Rectangle)
    .or_else(|| floating_shape_preset(office_shape.shape_type()))
  {
    Some(preset) => preset,
    None => {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        source,
      )?;
      a::ShapeTypeValues::Rectangle
    }
  };
  let insets = office_shape.text_insets();
  let drawing_id = media.next_drawing_id;
  media.next_drawing_id = media
    .next_drawing_id
    .checked_add(1)
    .ok_or_else(|| olecfsdk::Error::Limit("DOC drawing ID overflow".into()))?;
  let graphic = if let Some(image) = floating_image {
    let Some(content_type) = image_content_type(image) else {
      unsupported(
        report,
        options,
        ConversionCode::FloatingPictureNotMapped,
        source,
      )?;
      return Ok(None);
    };
    let relationship_id = format!("rIdOlecfImage{drawing_id}");
    media.pending.push(PendingImage {
      relationship_id: relationship_id.clone(),
      content_type,
      data: image.data,
    });
    floating_picture_graphic(
      office_shape,
      geometry,
      relationship_id,
      FloatingPictureStyle {
        fill: picture_fill,
        outline: shape_outline,
        source_rectangle: picture_crop,
      },
    )
  } else {
    floating_wordprocessing_shape_graphic(
      shape,
      office_shape,
      geometry,
      preset,
      FloatingWordprocessingStyle {
        fill: shape_fill,
        outline: shape_outline,
        insets,
      },
      textbox,
    )
  };
  report.record(Disposition::Mapped);
  Ok(Some(RunChoice::Drawing(Box::new(Drawing {
    drawing_choice: Some(DrawingChoice::Anchor(Box::new(wp::Anchor {
      distance_from_top,
      distance_from_bottom,
      distance_from_left,
      distance_from_right,
      simple_pos: Some(shape_source.simple_rectangle.into()),
      relative_height: Some(
        u32::try_from(office_shape.z_order())
          .map_err(|_| olecfsdk::Error::Limit("DOC OfficeArt z-order exceeds u32".into()))?,
      ),
      behind_doc: shape_source.below_text.into(),
      locked: shape_source.anchor_locked.into(),
      layout_in_cell: office_shape.layout_in_cell().into(),
      hidden: office_shape.hidden().then_some(true.into()),
      allow_overlap: office_shape.allow_overlap().into(),
      simple_position: Some(wp::SimplePosition {
        x: ooxmlsdk::simple_type::CoordinateValue::Emu(i64::from(
          if shape_source.simple_rectangle {
            geometry.left
          } else {
            0
          },
        )),
        y: ooxmlsdk::simple_type::CoordinateValue::Emu(i64::from(
          if shape_source.simple_rectangle {
            geometry.top
          } else {
            0
          },
        )),
      }),
      horizontal_position: Some(Box::new(wp::HorizontalPosition {
        relative_from: horizontal_relative,
        horizontal_position_choice: Some(wp::HorizontalPositionChoice::PositionOffset(
          geometry.left,
        )),
      })),
      vertical_position: Some(Box::new(wp::VerticalPosition {
        relative_from: vertical_relative,
        vertical_position_choice: Some(wp::VerticalPositionChoice::PositionOffset(geometry.top)),
      })),
      extent: wp::Extent {
        cx: i64::from(geometry.width),
        cy: i64::from(geometry.height),
      },
      anchor_choice: Some(anchor_choice),
      doc_properties: Some(Box::new(wp::DocProperties {
        id: drawing_id,
        name: if shape.story_text.is_some() {
          format!("Legacy Text Box {}", shape_source.shape_id)
        } else {
          format!("Legacy Shape {}", shape_source.shape_id)
        },
        ..Default::default()
      })),
      graphic: Box::new(graphic),
      ..Default::default()
    }))),
    ..Default::default()
  }))))
}

fn floating_wordprocessing_shape_graphic(
  shape: DocFloatingShape<'_>,
  office_shape: DocOfficeArtShapeRef<'_>,
  geometry: FloatingGeometry,
  preset: a::ShapeTypeValues,
  style: FloatingWordprocessingStyle,
  textbox: Option<wps::WordprocessingShapeChoice2>,
) -> a::Graphic {
  let shape_id = office_shape.shape().shape_id;
  a::Graphic {
    graphic_data: a::GraphicData {
      uri: "http://schemas.microsoft.com/office/word/2010/wordprocessingShape".into(),
      graphic_data_choice: vec![a::GraphicDataChoice::WordprocessingShape(Box::new(
        wps::WordprocessingShape {
          non_visual_drawing_properties: Some(Box::new(wps::NonVisualDrawingProperties {
            id: shape_id,
            name: format!("Legacy Shape {shape_id}"),
            ..Default::default()
          })),
          wordprocessing_shape_choice1: Some(
            if matches!(office_shape.shape_type(), 20 | 32..=40) {
              wps::WordprocessingShapeChoice::NonVisualConnectorProperties(Box::default())
            } else {
              wps::WordprocessingShapeChoice::NonVisualDrawingShapeProperties(Box::new(
                wps::NonVisualDrawingShapeProperties {
                  text_box: shape.story_text.is_some().then_some(true.into()),
                  ..Default::default()
                },
              ))
            },
          ),
          shape_properties: Some(Box::new(wps::ShapeProperties {
            transform2_d: Some(Box::new(floating_transform(office_shape, geometry))),
            shape_properties_choice1: Some(wps::ShapePropertiesChoice::PresetGeometry(Box::new(
              a::PresetGeometry {
                preset,
                adjust_value_list: Some(Default::default()),
                ..Default::default()
              },
            ))),
            shape_properties_choice2: style.fill,
            outline: style.outline,
            ..Default::default()
          })),
          wordprocessing_shape_choice2: textbox,
          text_body_properties: shape.story_text.map(|_| {
            Box::new(wps::TextBodyProperties {
              left_inset: Some(style.insets.left()),
              top_inset: Some(style.insets.top()),
              right_inset: Some(style.insets.right()),
              bottom_inset: Some(style.insets.bottom()),
              ..Default::default()
            })
          }),
          ..Default::default()
        },
      ))],
    },
    ..Default::default()
  }
}

fn floating_picture_graphic(
  office_shape: DocOfficeArtShapeRef<'_>,
  geometry: FloatingGeometry,
  relationship_id: String,
  style: FloatingPictureStyle,
) -> a::Graphic {
  let shape_id = office_shape.shape().shape_id;
  a::Graphic {
    graphic_data: a::GraphicData {
      uri: "http://schemas.openxmlformats.org/drawingml/2006/picture".into(),
      graphic_data_choice: vec![a::GraphicDataChoice::Picture(Box::new(pic::Picture {
        non_visual_picture_properties: Some(Box::new(pic::NonVisualPictureProperties {
          non_visual_drawing_properties: Box::new(pic::NonVisualDrawingProperties {
            id: shape_id,
            name: format!("Legacy Picture {shape_id}"),
            ..Default::default()
          }),
          non_visual_picture_drawing_properties: Box::default(),
        })),
        blip_fill: Some(Box::new(pic::BlipFill {
          blip: Some(Box::new(a::Blip {
            embed: Some(relationship_id),
            ..Default::default()
          })),
          source_rectangle: style.source_rectangle,
          blip_fill_choice: Some(pic::BlipFillChoice::Stretch(Box::new(a::Stretch {
            fill_rectangle: Some(Default::default()),
            ..Default::default()
          }))),
          ..Default::default()
        })),
        shape_properties: Some(Box::new(pic::ShapeProperties {
          transform2_d: Some(Box::new(floating_transform(office_shape, geometry))),
          shape_properties_choice1: Some(pic::ShapePropertiesChoice::PresetGeometry(Box::new(
            a::PresetGeometry {
              preset: a::ShapeTypeValues::Rectangle,
              adjust_value_list: Some(Default::default()),
              ..Default::default()
            },
          ))),
          shape_properties_choice2: style.fill,
          outline: style.outline,
          ..Default::default()
        })),
        ..Default::default()
      }))],
    },
    ..Default::default()
  }
}

fn floating_picture_crop(
  office_shape: DocOfficeArtShapeRef<'_>,
  formatting_loss: &mut bool,
) -> Option<a::SourceRectangle> {
  let crop = office_shape.picture_crop();
  let values = [crop.left(), crop.top(), crop.right(), crop.bottom()];
  if values.iter().all(|value| *value == 0) {
    return None;
  }
  let converted = values.map(fixed_16_16_to_drawingml_percentage);
  let [Some(left), Some(top), Some(right), Some(bottom)] = converted else {
    *formatting_loss = true;
    return None;
  };
  Some(a::SourceRectangle {
    left: Some(left),
    top: Some(top),
    right: Some(right),
    bottom: Some(bottom),
    ..Default::default()
  })
}

fn fixed_16_16_to_drawingml_percentage(
  value: i32,
) -> Option<ooxmlsdk::simple_type::DrawingmlPercentageValue> {
  let scaled = i64::from(value) * 100_000;
  let rounded = if scaled < 0 {
    (scaled - 32_768) / 65_536
  } else {
    (scaled + 32_768) / 65_536
  };
  i32::try_from(rounded)
    .ok()
    .map(ooxmlsdk::simple_type::DrawingmlPercentageValue::Decimal)
}

fn floating_transform(
  office_shape: DocOfficeArtShapeRef<'_>,
  geometry: FloatingGeometry,
) -> a::Transform2D {
  let shape_horizontal_flip = office_shape
    .shape()
    .flags
    .contains(olecfsdk::office_art::OfficeArtShapeFlags::FLIP_HORIZONTAL);
  let shape_vertical_flip = office_shape
    .shape()
    .flags
    .contains(olecfsdk::office_art::OfficeArtShapeFlags::FLIP_VERTICAL);
  a::Transform2D {
    horizontal_flip: (shape_horizontal_flip ^ geometry.reverse_horizontal).then_some(true.into()),
    vertical_flip: (shape_vertical_flip ^ geometry.reverse_vertical).then_some(true.into()),
    offset: Some(a::Offset {
      x: ooxmlsdk::simple_type::CoordinateValue::Emu(0),
      y: ooxmlsdk::simple_type::CoordinateValue::Emu(0),
    }),
    extents: Some(a::Extents {
      cx: ooxmlsdk::simple_type::CoordinateValue::Emu(i64::from(geometry.width)),
      cy: ooxmlsdk::simple_type::CoordinateValue::Emu(i64::from(geometry.height)),
    }),
    ..Default::default()
  }
}

fn floating_shape_geometry(
  anchor: DocShapeAnchorRef<'_>,
  shape_type: u16,
) -> Option<FloatingGeometry> {
  let rectangle = anchor.source().rectangle;
  let reverse_horizontal = rectangle.right < rectangle.left;
  let reverse_vertical = rectangle.bottom < rectangle.top;
  let left = rectangle.left.min(rectangle.right);
  let top = rectangle.top.min(rectangle.bottom);
  let width = i64::from(rectangle.right).abs_diff(i64::from(rectangle.left));
  let height = i64::from(rectangle.bottom).abs_diff(i64::from(rectangle.top));
  let is_line = matches!(shape_type, 20 | 32..=40);
  if (width == 0 || height == 0) && !is_line {
    return None;
  }
  let signed_to_emu = |value: i32| {
    i64::from(value)
      .checked_mul(ooxmlsdk::units::EMUS_PER_TWIP)
      .and_then(|value| i32::try_from(value).ok())
  };
  let unsigned_to_emu = |value: u64| {
    i64::try_from(value)
      .ok()?
      .checked_mul(ooxmlsdk::units::EMUS_PER_TWIP)
      .and_then(|value| i32::try_from(value).ok())
  };
  Some(FloatingGeometry {
    left: signed_to_emu(left)?,
    top: signed_to_emu(top)?,
    width: unsigned_to_emu(width)?,
    height: unsigned_to_emu(height)?,
    reverse_horizontal,
    reverse_vertical,
  })
}

const fn floating_shape_preset(shape_type: u16) -> Option<a::ShapeTypeValues> {
  use a::ShapeTypeValues as Shape;
  Some(match shape_type {
    1 | 202 => Shape::Rectangle,
    2 => Shape::RoundRectangle,
    3 => Shape::Ellipse,
    4 => Shape::Diamond,
    5 => Shape::Triangle,
    6 => Shape::RightTriangle,
    7 => Shape::Parallelogram,
    8 => Shape::Trapezoid,
    9 => Shape::Hexagon,
    10 => Shape::Octagon,
    11 => Shape::Plus,
    12 => Shape::Star5,
    13 | 14 => Shape::RightArrow,
    15 => Shape::HomePlate,
    19 => Shape::Arc,
    20 | 32 => Shape::Line,
    21 => Shape::Plaque,
    22 => Shape::Can,
    23 => Shape::Donut,
    55 => Shape::Chevron,
    56 => Shape::Pentagon,
    57 => Shape::NoSmoking,
    58 => Shape::Star8,
    59 => Shape::Star16,
    60 => Shape::Star32,
    61 => Shape::WedgeRectangleCallout,
    62 => Shape::WedgeRoundRectangleCallout,
    63 => Shape::WedgeEllipseCallout,
    64 => Shape::Wave,
    66 => Shape::LeftArrow,
    67 => Shape::DownArrow,
    68 => Shape::UpArrow,
    69 => Shape::LeftRightArrow,
    70 => Shape::UpDownArrow,
    73 => Shape::LightningBolt,
    74 => Shape::Heart,
    76 => Shape::QuadArrow,
    95 => Shape::BlockArc,
    96 => Shape::SmileyFace,
    99 => Shape::CircularArrow,
    _ => return None,
  })
}

fn floating_shape_fill(
  shape: DocOfficeArtShapeRef<'_>,
  loss: &mut bool,
) -> Option<wps::ShapePropertiesChoice2> {
  match shape.fill() {
    DocOfficeArtFill::None => Some(wps::ShapePropertiesChoice2::NoFill(a::NoFill::default())),
    DocOfficeArtFill::Solid(color) => floating_solid_fill(color).map_or_else(
      || {
        *loss = true;
        None
      },
      |fill| Some(wps::ShapePropertiesChoice2::SolidFill(Box::new(fill))),
    ),
    DocOfficeArtFill::Other { .. } => {
      *loss = true;
      None
    }
  }
}

fn floating_picture_fill(
  shape: DocOfficeArtShapeRef<'_>,
  loss: &mut bool,
) -> Option<pic::ShapePropertiesChoice2> {
  match shape.fill() {
    DocOfficeArtFill::None => Some(pic::ShapePropertiesChoice2::NoFill(a::NoFill::default())),
    DocOfficeArtFill::Solid(color) => floating_solid_fill(color).map_or_else(
      || {
        *loss = true;
        None
      },
      |fill| Some(pic::ShapePropertiesChoice2::SolidFill(Box::new(fill))),
    ),
    DocOfficeArtFill::Other { .. } => {
      *loss = true;
      None
    }
  }
}

fn floating_shape_outline(
  shape: DocOfficeArtShapeRef<'_>,
  loss: &mut bool,
) -> Option<Box<a::Outline>> {
  match shape.line() {
    DocOfficeArtLine::None => Some(Box::new(a::Outline {
      outline_choice1: Some(a::OutlineChoice::NoFill(a::NoFill::default())),
      ..Default::default()
    })),
    DocOfficeArtLine::Solid { color, width_emu } => {
      if !(0..=20_116_800).contains(&width_emu) {
        *loss = true;
        return None;
      }
      floating_solid_fill(color).map_or_else(
        || {
          *loss = true;
          None
        },
        |fill| {
          Some(Box::new(a::Outline {
            width: Some(width_emu),
            outline_choice1: Some(a::OutlineChoice::SolidFill(Box::new(fill))),
            ..Default::default()
          }))
        },
      )
    }
    DocOfficeArtLine::Other => {
      *loss = true;
      None
    }
  }
}

fn floating_solid_fill(color: DocOfficeArtColor) -> Option<a::SolidFill> {
  let DocOfficeArtColor::Rgb { red, green, blue } = color else {
    return None;
  };
  Some(a::SolidFill {
    solid_fill_choice: Some(a::SolidFillChoice::RgbColorModelHex(Box::new(
      a::RgbColorModelHex {
        val: format!("{red:02X}{green:02X}{blue:02X}"),
        ..Default::default()
      },
    ))),
    ..Default::default()
  })
}

fn floating_distance(value: i32, loss: &mut bool) -> Option<u32> {
  match u32::try_from(value) {
    Ok(value) => Some(value),
    Err(_) => {
      *loss = true;
      None
    }
  }
}

fn floating_horizontal_origin(
  source: &olecfsdk::doc::ShapeAnchor,
  report: &mut ConversionReport,
  options: ConversionOptions,
  location: SourceLocation,
) -> Result<wp::HorizontalRelativePositionValues> {
  if source.simple_rectangle {
    return Ok(wp::HorizontalRelativePositionValues::Page);
  }
  match source.horizontal_origin {
    0 => Ok(wp::HorizontalRelativePositionValues::Margin),
    1 => Ok(wp::HorizontalRelativePositionValues::Page),
    2 => Ok(wp::HorizontalRelativePositionValues::Column),
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        location,
      )?;
      Ok(wp::HorizontalRelativePositionValues::Page)
    }
  }
}

fn floating_vertical_origin(
  source: &olecfsdk::doc::ShapeAnchor,
  report: &mut ConversionReport,
  options: ConversionOptions,
  location: SourceLocation,
) -> Result<wp::VerticalRelativePositionValues> {
  if source.simple_rectangle {
    return Ok(wp::VerticalRelativePositionValues::Page);
  }
  match source.vertical_origin {
    0 => Ok(wp::VerticalRelativePositionValues::Margin),
    1 => Ok(wp::VerticalRelativePositionValues::Page),
    2 => Ok(wp::VerticalRelativePositionValues::Paragraph),
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        location,
      )?;
      Ok(wp::VerticalRelativePositionValues::Page)
    }
  }
}

fn floating_wrap(
  source: &olecfsdk::doc::ShapeAnchor,
  shape: DocOfficeArtShapeRef<'_>,
  report: &mut ConversionReport,
  options: ConversionOptions,
  location: SourceLocation,
) -> Result<wp::AnchorChoice> {
  let wrap_text = match source.wrap_side {
    0 => wp::WrapTextValues::BothSides,
    1 => wp::WrapTextValues::Left,
    2 => wp::WrapTextValues::Right,
    3 => wp::WrapTextValues::Largest,
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        location,
      )?;
      wp::WrapTextValues::BothSides
    }
  };
  match source.wrap_style {
    0 | 2 => Ok(wp::AnchorChoice::WrapSquare(Box::new(wp::WrapSquare {
      wrap_text,
      ..Default::default()
    }))),
    1 => Ok(wp::AnchorChoice::WrapTopBottom(Box::default())),
    3 => Ok(wp::AnchorChoice::WrapNone),
    4 => Ok(wp::AnchorChoice::WrapTight(Box::new(wp::WrapTight {
      wrap_text,
      wrap_polygon: Box::new(floating_wrap_polygon(shape, report, options, location)?),
      ..Default::default()
    }))),
    5 => Ok(wp::AnchorChoice::WrapThrough(Box::new(wp::WrapThrough {
      wrap_text,
      wrap_polygon: Box::new(floating_wrap_polygon(shape, report, options, location)?),
      ..Default::default()
    }))),
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        location,
      )?;
      Ok(wp::AnchorChoice::WrapSquare(Box::new(wp::WrapSquare {
        wrap_text,
        ..Default::default()
      })))
    }
  }
}

fn floating_wrap_polygon(
  shape: DocOfficeArtShapeRef<'_>,
  report: &mut ConversionReport,
  options: ConversionOptions,
  location: SourceLocation,
) -> Result<wp::WrapPolygon> {
  let polygon = shape.wrap_polygon()?;
  if let Some(polygon) = polygon {
    if polygon.len() < 2 {
      unsupported(
        report,
        options,
        ConversionCode::FloatingShapeGeometryNotMapped,
        location,
      )?;
    } else {
      let (start_x, start_y) = polygon
        .point(0)
        .expect("a nonempty OfficeArt wrapping polygon has a first point");
      let mut line_to = Vec::with_capacity(polygon.len() - 1);
      for index in 1..polygon.len() {
        let (x, y) = polygon
          .point(index)
          .expect("OfficeArt wrapping polygon index is in bounds");
        line_to.push(wp::LineTo { x, y });
      }
      return Ok(wp::WrapPolygon {
        edited: Some(true.into()),
        start_point: wp::StartPoint {
          x: start_x,
          y: start_y,
        },
        line_to,
      });
    }
  }
  Ok(wp::WrapPolygon {
    edited: Some(false.into()),
    start_point: wp::StartPoint { x: 0, y: 0 },
    line_to: vec![
      wp::LineTo { x: 21_600, y: 0 },
      wp::LineTo {
        x: 21_600,
        y: 21_600,
      },
      wp::LineTo { x: 0, y: 21_600 },
      wp::LineTo { x: 0, y: 0 },
    ],
  })
}

fn convert_textbox_content<'a>(
  text: DocTextRangeRef<'a>,
  fields: &mut DocFieldCursor,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
  source: SourceLocation,
) -> Result<TextBoxContent> {
  let tables = text.tables()?;
  let mut ranges = DocRangeCursor::new(Vec::new());
  let mut flow = DocFlowState {
    fields,
    ranges: &mut ranges,
    note_references: &[],
    comment_references: &[],
  };
  let mut choices = Vec::new();
  for block in text.blocks_with_tables(&tables)?.blocks() {
    let range = match block {
      DocBlockRef::Paragraph(paragraph) => paragraph.local_cp_range(),
      DocBlockRef::Table(table) => table.local_cp_range(),
    };
    if range.start < text.local_cp_range().start || range.end > text.local_cp_range().end {
      unsupported(
        report,
        options,
        ConversionCode::TextboxBoundaryNotMapped,
        source,
      )?;
    }
    choices.push(match block {
      DocBlockRef::Paragraph(paragraph) => {
        TextBoxContentChoice::Paragraph(Box::new(convert_paragraph(
          *paragraph,
          ParagraphContext::Textbox,
          &mut flow,
          options,
          report,
          media,
        )?))
      }
      DocBlockRef::Table(table) => TextBoxContentChoice::Table(Box::new(convert_table(
        table,
        &tables,
        ParagraphContext::TextboxTableCell,
        &mut flow,
        options,
        report,
        media,
      )?)),
    });
  }
  Ok(TextBoxContent {
    text_box_content_choice: choices,
  })
}

fn convert_special_content<'a>(
  document_part: DocDocumentPartRef<'a>,
  cp: DocCp,
  context: ParagraphContext,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Option<RunChoice>> {
  match document_part.special_content_at(cp)? {
    Some(DocSpecialContentRef::Picture { data_node, .. }) => {
      if matches!(
        context,
        ParagraphContext::Footnote
          | ParagraphContext::FootnoteTableCell
          | ParagraphContext::Endnote
          | ParagraphContext::EndnoteTableCell
          | ParagraphContext::Comment
          | ParagraphContext::CommentTableCell
      ) {
        unsupported(
          report,
          options,
          ConversionCode::InlinePictureNotMapped,
          source,
        )?;
        return Ok(None);
      }
      let DocDataNodeValue::Picture(picture) = &data_node.value else {
        unreachable!("typed picture relationships point at picture Data nodes")
      };
      convert_inline_picture(picture, source, options, report, media)
    }
    Some(DocSpecialContentRef::Binary { .. }) => {
      unsupported(
        report,
        options,
        ConversionCode::InlineBinaryNotMapped,
        source,
      )?;
      Ok(None)
    }
    Some(DocSpecialContentRef::OleObject { .. }) => {
      unsupported(report, options, ConversionCode::OleObjectNotMapped, source)?;
      Ok(None)
    }
    None => {
      unsupported(
        report,
        options,
        ConversionCode::ControlCharacterNotMapped,
        source,
      )?;
      Ok(None)
    }
  }
}

fn convert_inline_picture<'a>(
  picture: &'a PicfAndOfficeArtData,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
  media: &mut DocMediaState<'a>,
) -> Result<Option<RunChoice>> {
  let mut image = None;
  let mut image_count = 0usize;
  let mut opaque_metafile = false;
  picture.picture.visit(|record| {
    if let Some(candidate) = record.image_ref() {
      image_count += 1;
      if image.is_none() {
        image = Some(candidate);
      }
    } else if matches!(
        &record.data,
        OfficeArtRecordData::MetafileBlip(value)
            if matches!(value.file_data, OfficeArtMetafileData::Opaque { .. })
    ) {
      opaque_metafile = true;
    }
  });
  let Some(image) = image.filter(|_| image_count == 1 && !opaque_metafile) else {
    unsupported(
      report,
      options,
      ConversionCode::InlinePictureNotMapped,
      source,
    )?;
    return Ok(None);
  };
  let Some(content_type) = image_content_type(image) else {
    unsupported(
      report,
      options,
      ConversionCode::InlinePictureNotMapped,
      source,
    )?;
    return Ok(None);
  };
  let source_picture = picture.picf.picture;
  let Some(cx) = scaled_picture_extent(
    source_picture.goal_width_twips,
    source_picture.horizontal_scale_tenths_percent,
  ) else {
    unsupported(
      report,
      options,
      ConversionCode::InlinePictureNotMapped,
      source,
    )?;
    return Ok(None);
  };
  let Some(cy) = scaled_picture_extent(
    source_picture.goal_height_twips,
    source_picture.vertical_scale_tenths_percent,
  ) else {
    unsupported(
      report,
      options,
      ConversionCode::InlinePictureNotMapped,
      source,
    )?;
    return Ok(None);
  };
  let drawing_id = media.next_drawing_id;
  media.next_drawing_id = drawing_id
    .checked_add(1)
    .ok_or_else(|| olecfsdk::Error::Limit("DOC picture count exceeds u32".into()))?;
  let relationship_id = format!("rIdOlecfImage{drawing_id}");
  media.pending.push(PendingImage {
    relationship_id: relationship_id.clone(),
    content_type,
    data: image.data,
  });
  report.record(Disposition::Mapped);
  Ok(Some(RunChoice::Drawing(Box::new(Drawing {
    drawing_choice: Some(DrawingChoice::Inline(Box::new(wp::Inline {
      extent: wp::Extent { cx, cy },
      doc_properties: Box::new(wp::DocProperties {
        id: drawing_id,
        name: format!("Picture {drawing_id}"),
        ..Default::default()
      }),
      graphic: Box::new(a::Graphic {
        graphic_data: a::GraphicData {
          uri: "http://schemas.openxmlformats.org/drawingml/2006/picture".into(),
          graphic_data_choice: vec![a::GraphicDataChoice::Picture(Box::new(pic::Picture {
            non_visual_picture_properties: Some(Box::new(pic::NonVisualPictureProperties {
              non_visual_drawing_properties: Box::new(pic::NonVisualDrawingProperties {
                id: drawing_id,
                name: format!("Picture {drawing_id}"),
                ..Default::default()
              }),
              non_visual_picture_drawing_properties: Box::default(),
            })),
            blip_fill: Some(Box::new(pic::BlipFill {
              blip: Some(Box::new(a::Blip {
                embed: Some(relationship_id),
                ..Default::default()
              })),
              blip_fill_choice: Some(pic::BlipFillChoice::Stretch(Box::new(a::Stretch {
                fill_rectangle: Some(Default::default()),
                ..Default::default()
              }))),
              ..Default::default()
            })),
            shape_properties: Some(Box::new(pic::ShapeProperties {
              transform2_d: Some(Box::new(a::Transform2D {
                offset: Some(a::Offset {
                  x: ooxmlsdk::simple_type::CoordinateValue::Emu(0),
                  y: ooxmlsdk::simple_type::CoordinateValue::Emu(0),
                }),
                extents: Some(a::Extents {
                  cx: ooxmlsdk::simple_type::CoordinateValue::Emu(cx),
                  cy: ooxmlsdk::simple_type::CoordinateValue::Emu(cy),
                }),
                ..Default::default()
              })),
              shape_properties_choice1: Some(pic::ShapePropertiesChoice::PresetGeometry(Box::new(
                a::PresetGeometry {
                  preset: a::ShapeTypeValues::Rectangle,
                  adjust_value_list: Some(Default::default()),
                  ..Default::default()
                },
              ))),
              ..Default::default()
            })),
            ..Default::default()
          }))],
        },
        ..Default::default()
      }),
      ..Default::default()
    }))),
    ..Default::default()
  }))))
}

const fn image_content_type(image: OfficeArtImageRef<'_>) -> Option<&'static str> {
  match image.format {
    OfficeArtImageFormat::Emf => Some("image/x-emf"),
    OfficeArtImageFormat::Wmf => Some("image/x-wmf"),
    OfficeArtImageFormat::Jpeg => Some("image/jpeg"),
    OfficeArtImageFormat::Png => Some("image/png"),
    OfficeArtImageFormat::Tiff => Some("image/tiff"),
    OfficeArtImageFormat::Pict | OfficeArtImageFormat::Dib => None,
  }
}

fn scaled_picture_extent(goal_twips: i16, scale_tenths_percent: u16) -> Option<i64> {
  let goal_twips = i64::from(goal_twips);
  if goal_twips <= 0 || scale_tenths_percent == 0 {
    return None;
  }
  goal_twips
    .checked_mul(i64::from(scale_tenths_percent))?
    .checked_mul(ooxmlsdk::units::EMUS_PER_TWIP)?
    .checked_add(500)
    .map(|value| value / 1_000)
    .filter(|value| *value <= i64::from(i32::MAX))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParagraphContext {
  Body,
  TableCell,
  Footnote,
  FootnoteTableCell,
  Endnote,
  EndnoteTableCell,
  Comment,
  CommentTableCell,
  Textbox,
  TextboxTableCell,
}

impl ParagraphContext {
  const fn is_table_cell(self) -> bool {
    matches!(
      self,
      Self::TableCell
        | Self::FootnoteTableCell
        | Self::EndnoteTableCell
        | Self::CommentTableCell
        | Self::TextboxTableCell
    )
  }
}

fn text_type(value: &str) -> TextType {
  let preserve_space = value.starts_with(char::is_whitespace)
    || value.ends_with(char::is_whitespace)
    || value.contains("  ");
  TextType {
    space: preserve_space.then_some(SpaceProcessingModeValues::Preserve),
    xml_content: Some(value.to_owned()),
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

fn location(part: FieldDocumentPart, range: DocCpRange) -> SourceLocation {
  SourceLocation::DocRange {
    part,
    start_cp: range.start.value(),
    end_cp: range.end.value(),
  }
}

#[cfg(test)]
mod tests {
  use super::bookmark_column_range;

  #[test]
  fn bookmark_column_limit_becomes_inclusive_ooxml_last_column() {
    let properties = olecfsdk::doc::BookmarkStart {
      end_index: 0,
      column_start: 1,
      published: false,
      column_limit: 3,
      native: false,
      column: true,
    };
    assert_eq!(bookmark_column_range(&properties), Some((1, 2)));
  }

  #[test]
  fn bookmark_empty_column_interval_is_not_mapped_as_a_range() {
    let properties = olecfsdk::doc::BookmarkStart {
      end_index: 0,
      column_start: 2,
      published: false,
      column_limit: 2,
      native: false,
      column: true,
    };
    assert_eq!(bookmark_column_range(&properties), None);
  }
}
