use olecfsdk::{
  io::SdkEnumValue,
  office_art::{OfficeArtClientAnchor, OfficeArtImageFormat, OfficeArtShapeFlags},
  xls::{
    AutoFilter12DateGrouping, AutoFilter12DateGroupingLevel, AutoFilter12DynamicFilter,
    AutoFilter12Filter, AutoFilter12Record, AutoFilterOperand, AutoFilterOperandValue,
    AutoFilterRecord, BiffRecordData, BiffSubstreamKind, BiffUnicodeString, CellErrorCode,
    CellRange, CfColor, ColInfoRecord, EmptyRecordKind, EnhancedProtectionFlags, ExtFontScheme,
    ExtPropertyData, ExtRstBody, ExtendedHeaderFooterFlags, FeatureData, FeatureHeaderData,
    FixedF64RecordKind, FixedU16RecordKind, FontAttributes, FontRecord, FormulaOperator,
    FormulaTokenData, FormulaTokenStream, FullColorExt, HeaderFooterRecord, KpiSet, NameValue,
    PaneRecord, PhoneticAlignment, PhoneticType, PlvFlags, PlvRecord, PrintSetupRecord, Rfx,
    RowRecord, SclRecord, SelectionRecord, SheetExtRecord, SortConditionData, SortDataRecord,
    SortFieldParent, SstExtensionData, SstString, Window2Extension, Window2Flags, Window2Record,
    XfExtRecord, XfRecord, XlStringCharacters, XlsCellValue, XlsCellValueRef, XlsFile,
    XlsFormulaCachedValue, XlsFormulaDefinitionRef, XlsFormulaRef, XlsHyperlinkTarget,
    XlsPictureImageLink, XlsPictureRef, XlsSheetRef, XlsWorkbookView,
  },
};
use ooxmlsdk::{
  common::XmlNamespace,
  namespaces::XmlKnownNamespace,
  parts::spreadsheet_document::SpreadsheetDocument,
  parts::{
    drawings_part::DrawingsPart, image_part::ImagePart,
    shared_string_table_part::SharedStringTablePart, workbook_part::WorkbookPart,
    workbook_styles_part::WorkbookStylesPart, worksheet_comments_part::WorksheetCommentsPart,
    worksheet_part::WorksheetPart,
  },
  schemas::{
    opc_relationships::TargetMode,
    schemas_openxmlformats_org_drawingml_2006_main as a,
    schemas_openxmlformats_org_drawingml_2006_spreadsheet_drawing as xdr,
    schemas_openxmlformats_org_spreadsheetml_2006_main::{
      self as x, Author, Authors, BookViews, Break, CalculateModeValues, CalculationProperties,
      Cell, CellCommentsValues, CellFormula, CellValue, CellValues, Column, ColumnBreaks, Columns,
      Comment, CommentList, CommentText, Comments, EvenFooter, EvenHeader, FirstFooter,
      FirstHeader, HeaderFooter, Hyperlink, Hyperlinks, MergeCell, MergeCells, ObjectDisplayValues,
      OddFooter, OddHeader, OrientationValues, OutlineProperties, PageMargins, PageOrderValues,
      PageSetup, PageSetupProperties, Pane, PaneStateValues, PaneValues, PhoneticAlignmentValues,
      PhoneticProperties, PhoneticValues, PrintErrorValues, PrintOptions, ProtectedRange,
      ProtectedRanges, ReferenceModeValues, Row, RowBreaks, Selection, SharedStringItem,
      SharedStringTable, Sheet, SheetCalculationProperties, SheetData, SheetDimension,
      SheetFormatProperties, SheetProperties, SheetProtection, SheetStateValues, SheetView,
      SheetViewValues, SheetViews, Sheets, TabColor, Text, UpdateLinksBehaviorValues,
      VisibilityValues, Workbook, WorkbookProperties, WorkbookProtection, WorkbookView, Worksheet,
      XstringType,
    },
    xml::SpaceProcessingModeValues,
  },
  sdk::SpreadsheetDocumentType,
  simple_type::{BooleanValue, CoordinateValue},
};

use crate::{
  ConversionCode, ConversionOptions, ConversionOutput, ConversionReport, Disposition, Error,
  LossPolicy, Result, SourceLocation, metadata::convert_core_properties,
};

#[derive(Default)]
struct XlsMediaState {
  parts: Vec<(u32, ImagePart)>,
}

/// Converts a typed BIFF8 workbook root into a SpreadsheetML package.
///
/// The default policy rejects the first known semantic loss. Use
/// [`convert_xls_with_options`] to request an explicit diagnostic report.
pub fn convert_xls(source: &XlsFile) -> Result<ConversionOutput<SpreadsheetDocument>> {
  convert_xls_with_options(source, ConversionOptions::default())
}

/// Converts a typed BIFF8 workbook root with an explicit loss policy.
pub fn convert_xls_with_options(
  source: &XlsFile,
  options: ConversionOptions,
) -> Result<ConversionOutput<SpreadsheetDocument>> {
  let mut report = ConversionReport::default();
  let workbook = source
    .workbooks
    .first()
    .ok_or_else(|| olecfsdk::Error::invalid(0, "XLS has no Workbook stream"))?;
  for workbook_index in 1..source.workbooks.len() {
    unsupported(
      &mut report,
      options,
      ConversionCode::AdditionalWorkbookStreamNotMapped,
      SourceLocation::XlsWorkbook { workbook_index },
    )?;
  }
  let view = workbook.relationships()?;
  if has_unmapped_workbook_features(source, &view) {
    unsupported(
      &mut report,
      options,
      ConversionCode::WorkbookFeatureNotMapped,
      SourceLocation::XlsWorkbook { workbook_index: 0 },
    )?;
  }
  let mut next_target_sheet = 0_u32;
  let target_sheet_positions = view
    .sheets()
    .iter()
    .map(|sheet| {
      if sheet.kind() == BiffSubstreamKind::WorksheetOrDialogSheet
        && sheet.metadata().sheet_type == 0
      {
        let position = next_target_sheet;
        next_target_sheet += 1;
        Some(position)
      } else {
        None
      }
    })
    .collect::<Vec<_>>();
  let sheet_windows = view
    .sheets()
    .iter()
    .copied()
    .map(collect_sheet_window_groups)
    .collect::<Vec<_>>();
  let workbook_views = convert_workbook_views(
    &view,
    &target_sheet_positions,
    &sheet_windows,
    options,
    &mut report,
  )?;
  let workbook_view_count = workbook_views.len();
  let workbook_properties = convert_workbook_properties(&view, options, &mut report)?;
  let workbook_protection = convert_workbook_protection(&view, options, &mut report)?;
  let calculation_properties = convert_calculation_properties(&view, options, &mut report)?;

  let mut document = SpreadsheetDocument::create(SpreadsheetDocumentType::Workbook);
  let workbook_part = document.add_new_part_auto_id::<WorkbookPart>()?;
  let styles = convert_stylesheet(&view)?;
  let source_pictures = view.pictures()?;
  let mut media = XlsMediaState::default();
  let mut target_sheets = Vec::with_capacity(view.sheets().len());
  for (sheet_index, source_sheet) in view.sheets().iter().copied().enumerate() {
    let source_location = SourceLocation::XlsSheet {
      workbook_index: 0,
      sheet_index,
    };
    if source_sheet.kind() != BiffSubstreamKind::WorksheetOrDialogSheet
      || source_sheet.metadata().sheet_type != 0
    {
      unsupported(
        &mut report,
        options,
        ConversionCode::SheetKindNotMapped,
        source_location,
      )?;
      continue;
    }

    let sheet_properties = convert_sheet_properties(
      source_sheet,
      &sheet_windows[sheet_index],
      sheet_index,
      options,
      &mut report,
    )?;
    let sheet_dimension = convert_sheet_dimension(source_sheet, sheet_index, options, &mut report)?;
    let sheet_format_properties =
      convert_sheet_format_properties(source_sheet, sheet_index, options, &mut report)?;
    let sheet_views = convert_sheet_views(
      &sheet_windows[sheet_index],
      workbook_view_count,
      sheet_index,
      options,
      &mut report,
    )?;
    let page_settings = convert_page_settings(source_sheet, sheet_index, options, &mut report)?;
    let sheet_calculation_properties =
      convert_sheet_calculation_properties(source_sheet, sheet_index, options, &mut report)?;
    let sheet_protection =
      convert_sheet_protection(source_sheet, sheet_index, options, &mut report)?;
    let protected_ranges =
      convert_protected_ranges(source_sheet, sheet_index, options, &mut report)?;
    let phonetic =
      convert_phonetic_information(&view, source_sheet, sheet_index, options, &mut report)?;
    let sort_and_filter =
      convert_sheet_sort_and_filter(&view, source_sheet, sheet_index, options, &mut report)?;
    let index = source_sheet.sparse_cell_index()?;
    let columns = convert_columns(
      source_sheet.column_infos(),
      sheet_index,
      options,
      &mut report,
      &styles.xf_unmapped,
    )?;
    let merge_cell = source_sheet
      .merged_cells()
      .map(|range| MergeCell {
        reference: cell_range_reference(
          range.first_row,
          range.first_column,
          range.last_row,
          range.last_column,
        ),
      })
      .collect::<Vec<_>>();
    let merge_cells = if merge_cell.is_empty() {
      None
    } else {
      Some(MergeCells {
        count: Some(
          u32::try_from(merge_cell.len())
            .map_err(|_| olecfsdk::Error::Limit("XLS merged range count exceeds u32".into()))?,
        ),
        merge_cell,
      })
    };
    let source_hyperlinks = source_sheet.hyperlinks()?;
    let source_comments = source_sheet.comments()?;
    let cell_context = XlsCellConversionContext {
      view: &view,
      index: &index,
      sheet_index,
      options,
      xf_unmapped: &styles.xf_unmapped,
      phonetic_visible_ranges: &phonetic.visible_ranges,
    };
    let mut rows = Vec::new();
    for source_row in index.rows() {
      let mut cells = Vec::new();
      for source_cell in source_row.cells() {
        cells.push(convert_cell(&cell_context, source_cell, &mut report)?);
      }
      rows.push(convert_row(
        source_row.row(),
        source_row.definition()?,
        cells,
        sheet_index,
        options,
        &mut report,
        &styles.xf_unmapped,
      )?);
      report.record(Disposition::Mapped);
    }

    let worksheet_part = workbook_part.add_new_part_auto_id::<_, WorksheetPart>(&mut document)?;
    let mut hyperlinks = Vec::with_capacity(source_hyperlinks.len());
    for source_hyperlink in source_hyperlinks {
      let range = source_hyperlink.value();
      let reference = cell_range_reference(
        range.first_row,
        range.first_column,
        range.last_row,
        range.last_column,
      );
      if source_hyperlink.target_frame_name.is_some() {
        unsupported(
          &mut report,
          options,
          ConversionCode::HyperlinkFrameNotMapped,
          source_location,
        )?;
      }
      let target = match source_hyperlink.target {
        Some(XlsHyperlinkTarget::String(value) | XlsHyperlinkTarget::Url(value)) => Some(value),
        Some(XlsHyperlinkTarget::File {
          long_path: Some(value),
          ..
        }) => Some(value),
        Some(
          XlsHyperlinkTarget::File {
            long_path: None, ..
          }
          | XlsHyperlinkTarget::Standard { .. },
        ) => {
          unsupported(
            &mut report,
            options,
            ConversionCode::HyperlinkTargetNotMapped,
            source_location,
          )?;
          None
        }
        None => None,
      };
      let id = target
        .map(|target| {
          worksheet_part
            .add_hyperlink_relationship_auto_id(&mut document, target, TargetMode::External)
            .map(|relationship| relationship.id().to_owned())
        })
        .transpose()?;
      hyperlinks.push(Hyperlink {
        reference,
        id,
        location: source_hyperlink.location,
        display: source_hyperlink.display_name,
        ..Default::default()
      });
      report.record(Disposition::Mapped);
    }
    let drawing = convert_sheet_pictures(
      source_pictures
        .iter()
        .copied()
        .filter(|picture| picture.sheet().id() == source_sheet.id()),
      sheet_index,
      &worksheet_part,
      &mut document,
      &mut media,
      options,
      &mut report,
    )?;
    worksheet_part.set_root_element(
      &mut document,
      Worksheet {
        xmlns: vec![XmlNamespace::known(XmlKnownNamespace::R)],
        sheet_properties,
        sheet_dimension,
        sheet_views,
        sheet_format_properties,
        columns: (!columns.is_empty())
          .then_some(Columns { column: columns })
          .into_iter()
          .collect(),
        sheet_data: SheetData { row: rows },
        sheet_calculation_properties,
        sheet_protection,
        protected_ranges,
        phonetic_properties: phonetic.properties,
        auto_filter: sort_and_filter.auto_filter,
        sort_state: sort_and_filter.sort_state,
        merge_cells,
        hyperlinks: (!hyperlinks.is_empty()).then_some(Hyperlinks {
          hyperlink: hyperlinks,
        }),
        print_options: page_settings.print_options,
        page_margins: page_settings.page_margins,
        page_setup: page_settings.page_setup,
        header_footer: page_settings.header_footer,
        row_breaks: page_settings.row_breaks,
        column_breaks: page_settings.column_breaks,
        drawing,
        ..Default::default()
      },
    )?;
    if let Some(comments) = convert_comments(source_comments, sheet_index, options, &mut report)? {
      let comments_part =
        worksheet_part.add_new_part_auto_id::<_, WorksheetCommentsPart>(&mut document)?;
      comments_part.set_root_element(&mut document, comments)?;
    }
    let relationship_id = workbook_part
      .get_id_of_part(&document, &worksheet_part)
      .expect("a newly added worksheet has a relationship id")
      .to_owned();
    let state = convert_sheet_state(
      source_sheet.metadata().state,
      source_location,
      options,
      &mut report,
    )?;
    target_sheets.push(Sheet {
      name: source_sheet.metadata().name.value.clone(),
      sheet_id: u32::try_from(sheet_index + 1)
        .map_err(|_| olecfsdk::Error::Limit("XLS sheet index exceeds u32".into()))?,
      state,
      id: relationship_id,
      ..Default::default()
    });
    if has_unmapped_worksheet_features(source_sheet) {
      unsupported(
        &mut report,
        options,
        ConversionCode::WorksheetFeatureNotMapped,
        source_location,
      )?;
    }
    report.record(Disposition::Mapped);
  }

  workbook_part.set_root_element(
    &mut document,
    Workbook {
      xmlns: vec![XmlNamespace::known(XmlKnownNamespace::R)],
      workbook_properties,
      workbook_protection,
      book_views: (!workbook_views.is_empty()).then_some(BookViews {
        workbook_view: workbook_views,
      }),
      sheets: Sheets {
        sheet: target_sheets,
      },
      calculation_properties,
      ..Default::default()
    },
  )?;
  if let Some(shared_strings) = convert_shared_strings(&view, options, &mut report)? {
    let part = workbook_part.add_new_part_auto_id::<_, SharedStringTablePart>(&mut document)?;
    part.set_root_element(&mut document, shared_strings)?;
  }
  let styles_part = workbook_part.add_new_part_auto_id::<_, WorkbookStylesPart>(&mut document)?;
  styles_part.set_root_element(&mut document, styles.root)?;
  report.record(Disposition::Mapped);
  if let Some(properties) = convert_core_properties(&source.shared, options, &mut report)? {
    let properties_part = document.add_core_file_properties_part()?;
    properties_part.set_root_element(&mut document, properties)?;
  }
  Ok(ConversionOutput { document, report })
}

fn convert_sheet_pictures<'a>(
  source: impl Iterator<Item = XlsPictureRef<'a>>,
  sheet_index: usize,
  worksheet_part: &WorksheetPart,
  document: &mut SpreadsheetDocument,
  media: &mut XlsMediaState,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<x::Drawing>> {
  let mut source = source.peekable();
  if source.peek().is_none() {
    return Ok(None);
  }
  let mut drawing_part = None;
  let mut anchors = Vec::new();
  for picture in source {
    let location = SourceLocation::XlsDrawing {
      workbook_index: 0,
      sheet_index,
      shape_id: picture.shape().shape_id,
    };
    let image = match picture.image() {
      XlsPictureImageLink::Resolved(image) => image,
      XlsPictureImageLink::Delayed { .. }
      | XlsPictureImageLink::Unsupported
      | XlsPictureImageLink::Missing => {
        unsupported(
          report,
          options,
          ConversionCode::SpreadsheetPictureNotMapped,
          location,
        )?;
        continue;
      }
    };
    let Some(content_type) = xls_image_content_type(image.format) else {
      unsupported(
        report,
        options,
        ConversionCode::SpreadsheetPictureNotMapped,
        location,
      )?;
      continue;
    };
    let OfficeArtClientAnchor::Words18 { flags, coordinates } = picture.anchor() else {
      unsupported(
        report,
        options,
        ConversionCode::SpreadsheetPictureAnchorNotMapped,
        location,
      )?;
      continue;
    };
    let edit_as = match flags {
      0 => xdr::EditAsValues::TwoCell,
      2 => xdr::EditAsValues::OneCell,
      3 => xdr::EditAsValues::Absolute,
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::SpreadsheetPictureAnchorNotMapped,
          location,
        )?;
        xdr::EditAsValues::TwoCell
      }
    };
    if coordinates[1] > 1_023
      || coordinates[3] > 255
      || coordinates[5] > 1_023
      || coordinates[7] > 255
    {
      unsupported(
        report,
        options,
        ConversionCode::SpreadsheetPictureAnchorNotMapped,
        location,
      )?;
    }
    if drawing_part.is_none() {
      let part = worksheet_part.add_new_part_auto_id::<_, DrawingsPart>(document)?;
      let relationship_id = worksheet_part
        .get_id_of_part(document, &part)
        .expect("a newly added drawing part has a relationship ID")
        .to_owned();
      drawing_part = Some((part, relationship_id));
    }
    let Some((drawings_part, _)) = drawing_part.as_ref() else {
      unreachable!("the XLS drawing part was initialized immediately above")
    };
    let relationship_id = add_xls_image_relationship(
      picture.blip_identifier(),
      image,
      content_type,
      drawings_part,
      document,
      media,
    )?;
    let mut formatting_loss = picture.shape_type() != 75;
    let source_rectangle = xls_picture_crop(picture, &mut formatting_loss);
    if formatting_loss {
      unsupported(
        report,
        options,
        ConversionCode::SpreadsheetPictureFormattingNotMapped,
        location,
      )?;
    }
    let shape_flags = picture.shape().flags;
    let transform2_d = (shape_flags
      .intersects(OfficeArtShapeFlags::FLIP_HORIZONTAL | OfficeArtShapeFlags::FLIP_VERTICAL))
    .then(|| {
      Box::new(a::Transform2D {
        horizontal_flip: shape_flags
          .contains(OfficeArtShapeFlags::FLIP_HORIZONTAL)
          .then_some(true.into()),
        vertical_flip: shape_flags
          .contains(OfficeArtShapeFlags::FLIP_VERTICAL)
          .then_some(true.into()),
        ..Default::default()
      })
    });
    anchors.push(xdr::WorksheetDrawingChoice::TwoCellAnchor(Box::new(
      xdr::TwoCellAnchor {
        edit_as: Some(edit_as),
        from_marker: Box::new(xls_from_marker(coordinates)),
        to_marker: Box::new(xls_to_marker(coordinates)),
        two_cell_anchor_choice: Some(xdr::TwoCellAnchorChoice::Picture(Box::new(xdr::Picture {
          non_visual_picture_properties: Box::new(xdr::NonVisualPictureProperties {
            non_visual_drawing_properties: Box::new(xdr::NonVisualDrawingProperties {
              id: picture.shape().shape_id,
              name: format!("Legacy Picture {}", picture.shape().shape_id),
              ..Default::default()
            }),
            non_visual_picture_drawing_properties: Box::default(),
          }),
          blip_fill: Some(Box::new(xdr::BlipFill {
            blip: Some(Box::new(a::Blip {
              embed: Some(relationship_id),
              ..Default::default()
            })),
            source_rectangle,
            blip_fill_choice: Some(xdr::BlipFillChoice::Stretch(Box::new(a::Stretch {
              fill_rectangle: Some(Default::default()),
              ..Default::default()
            }))),
            ..Default::default()
          })),
          shape_properties: Box::new(xdr::ShapeProperties {
            transform2_d,
            shape_properties_choice1: Some(xdr::ShapePropertiesChoice::PresetGeometry(Box::new(
              a::PresetGeometry {
                preset: a::ShapeTypeValues::Rectangle,
                adjust_value_list: Some(Default::default()),
                ..Default::default()
              },
            ))),
            shape_properties_choice2: Some(xdr::ShapePropertiesChoice2::NoFill(
              a::NoFill::default(),
            )),
            outline: Some(Box::new(a::Outline {
              outline_choice1: Some(a::OutlineChoice::NoFill(a::NoFill::default())),
              ..Default::default()
            })),
            ..Default::default()
          }),
          ..Default::default()
        }))),
        client_data: xdr::ClientData::default(),
      },
    )));
    report.record(Disposition::Mapped);
  }
  let Some((drawings_part, relationship_id)) = drawing_part else {
    return Ok(None);
  };
  drawings_part.set_root_element(
    document,
    xdr::WorksheetDrawing {
      xmlns: vec![
        XmlNamespace::known(XmlKnownNamespace::A),
        XmlNamespace::known(XmlKnownNamespace::R),
      ],
      worksheet_drawing_choice: anchors,
    },
  )?;
  Ok(Some(x::Drawing {
    xmlns: vec![XmlNamespace::known(XmlKnownNamespace::R)],
    id: relationship_id,
  }))
}

fn add_xls_image_relationship(
  blip_identifier: u32,
  source: olecfsdk::office_art::OfficeArtImageRef<'_>,
  content_type: &'static str,
  host: &DrawingsPart,
  document: &mut SpreadsheetDocument,
  media: &mut XlsMediaState,
) -> Result<String> {
  let image_part = if let Some((_, part)) = media
    .parts
    .iter()
    .find(|(identifier, _)| *identifier == blip_identifier)
  {
    host.add_part(document, part.clone())?
  } else {
    let part = host.add_image_part(document, content_type)?;
    part.set_data(document, source.data.to_vec())?;
    media.parts.push((blip_identifier, part.clone()));
    part
  };
  Ok(
    host
      .get_id_of_part(document, &image_part)
      .expect("a newly related XLS image has a relationship ID")
      .to_owned(),
  )
}

fn xls_from_marker(coordinates: [u16; 8]) -> xdr::FromMarker {
  xdr::FromMarker {
    column_id: i32::from(coordinates[0]),
    column_offset: xls_column_offset(coordinates[1]),
    row_id: i32::from(coordinates[2]),
    row_offset: xls_row_offset(coordinates[3]),
  }
}

fn xls_to_marker(coordinates: [u16; 8]) -> xdr::ToMarker {
  xdr::ToMarker {
    column_id: i32::from(coordinates[4]),
    column_offset: xls_column_offset(coordinates[5]),
    row_id: i32::from(coordinates[6]),
    row_offset: xls_row_offset(coordinates[7]),
  }
}

fn xls_column_offset(value: u16) -> CoordinateValue {
  // BIFF client-anchor dx is 1/1024 of the host column. The current XLSX
  // vertical slice uses Excel's 64-pixel default column until dimensions
  // are mapped, so retain that same physical fraction in EMUs.
  CoordinateValue::Emu(i64::from(value) * 64 * 9_525 / 1_024)
}

fn xls_row_offset(value: u16) -> CoordinateValue {
  // BIFF client-anchor dy is 1/256 of the host row; 15 points is the Excel
  // default row height used by the current target worksheet.
  CoordinateValue::Emu(i64::from(value) * 15 * 12_700 / 256)
}

fn xls_picture_crop(
  picture: XlsPictureRef<'_>,
  formatting_loss: &mut bool,
) -> Option<a::SourceRectangle> {
  let crop = picture.crop();
  let values = [crop.left(), crop.top(), crop.right(), crop.bottom()];
  if values.iter().all(|value| *value == 0) {
    return None;
  }
  let converted = values.map(fixed_16_16_to_percentage);
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

fn fixed_16_16_to_percentage(
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

const fn xls_image_content_type(format: OfficeArtImageFormat) -> Option<&'static str> {
  match format {
    OfficeArtImageFormat::Emf => Some("image/x-emf"),
    OfficeArtImageFormat::Wmf => Some("image/x-wmf"),
    OfficeArtImageFormat::Jpeg => Some("image/jpeg"),
    OfficeArtImageFormat::Png => Some("image/png"),
    OfficeArtImageFormat::Tiff => Some("image/tiff"),
    OfficeArtImageFormat::Pict | OfficeArtImageFormat::Dib => None,
  }
}

fn convert_comments(
  source: Vec<olecfsdk::xls::XlsCommentRef<'_>>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Comments>> {
  if source.is_empty() {
    return Ok(None);
  }
  let mut author_values = Vec::<String>::new();
  let mut comments = Vec::with_capacity(source.len());
  for comment in source {
    let row = comment.note().row;
    let column = comment.note().column;
    let location = SourceLocation::XlsCell {
      workbook_index: 0,
      sheet_index,
      row,
      column,
    };
    unsupported(
      report,
      options,
      ConversionCode::CommentFormattingNotMapped,
      location,
    )?;
    let author_id = if let Some(index) = author_values
      .iter()
      .position(|author| author == &comment.author)
    {
      u32::try_from(index)
        .map_err(|_| olecfsdk::Error::Limit("XLS comment author index exceeds u32".into()))?
    } else {
      let index = u32::try_from(author_values.len())
        .map_err(|_| olecfsdk::Error::Limit("XLS comment author count exceeds u32".into()))?;
      author_values.push(comment.author);
      index
    };
    comments.push(Comment {
      reference: cell_reference(row, column),
      author_id,
      comment_text: Box::new(CommentText {
        text: Some(Text(xstring(comment.content))),
        ..Default::default()
      }),
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  Ok(Some(Comments {
    xmlns: vec![XmlNamespace::known(XmlKnownNamespace::X)],
    authors: Authors {
      author: author_values
        .into_iter()
        .map(|value| Author(xstring(value)))
        .collect(),
    },
    comment_list: CommentList { comment: comments },
    ..Default::default()
  }))
}

fn has_unmapped_workbook_features(source: &XlsFile, view: &XlsWorkbookView<'_>) -> bool {
  !view.unresolved_sheets().is_empty()
    || !view.unlinked_substreams().is_empty()
    || !view.supporting_links().is_empty()
    || !view.external_sheets().is_empty()
    || !view.defined_names().is_empty()
    || !view.pivot_cache_definitions().is_empty()
    || !view.custom_views().is_empty()
    || !source.pivot_caches.is_empty()
    || source.revision_log.is_some()
    || source.user_names.is_some()
}

fn has_unmapped_worksheet_features(source: XlsSheetRef<'_>) -> bool {
  source
    .direct_records()
    .any(|record| !worksheet_record_is_accounted_for(&record.data))
}

/// Records listed here are either mapped directly, consumed by the typed
/// relationship/grid projections, or diagnosed by a more specific loss code.
/// Everything else remains an explicit worksheet-level loss when it actually
/// occurs in the source substream.
fn worksheet_record_is_accounted_for(record: &BiffRecordData) -> bool {
  match record {
    BiffRecordData::Bof(_)
    | BiffRecordData::Eof
    | BiffRecordData::Index(_)
    | BiffRecordData::DbCell(_)
    | BiffRecordData::EntExU2(_)
    | BiffRecordData::Dimensions(_)
    | BiffRecordData::WsBool(_)
    | BiffRecordData::CodeName(_)
    | BiffRecordData::SheetExt(_)
    | BiffRecordData::Sync(_)
    | BiffRecordData::DefaultRowHeight(_)
    | BiffRecordData::Guts(_)
    | BiffRecordData::ColInfo(_)
    | BiffRecordData::Window2(_)
    | BiffRecordData::Scl(_)
    | BiffRecordData::Plv(_)
    | BiffRecordData::Pane(_)
    | BiffRecordData::Selection(_)
    | BiffRecordData::PrintSetup(_)
    | BiffRecordData::Header(_)
    | BiffRecordData::Footer(_)
    | BiffRecordData::HorizontalPageBreaks(_)
    | BiffRecordData::VerticalPageBreaks(_)
    | BiffRecordData::PhoneticInfo(_)
    | BiffRecordData::Formula(_)
    | BiffRecordData::Formula4Compatibility(_)
    | BiffRecordData::SharedFormula(_)
    | BiffRecordData::Array(_)
    | BiffRecordData::Table(_)
    | BiffRecordData::StringValue(_)
    | BiffRecordData::Blank(_)
    | BiffRecordData::Number(_)
    | BiffRecordData::BoolErr(_)
    | BiffRecordData::Label(_)
    | BiffRecordData::LabelSst(_)
    | BiffRecordData::Rk(_)
    | BiffRecordData::MulRk(_)
    | BiffRecordData::MulBlank(_)
    | BiffRecordData::Row(_)
    | BiffRecordData::MergeCells(_)
    | BiffRecordData::Hyperlink(_)
    | BiffRecordData::AutoFilter(_)
    | BiffRecordData::AutoFilter12(_)
    | BiffRecordData::SortData(_)
    | BiffRecordData::FixedF64Bits { .. } => true,
    BiffRecordData::FixedU16 { kind, .. } => matches!(
      kind,
      FixedU16RecordKind::CalcCount
        | FixedU16RecordKind::CalcMode
        | FixedU16RecordKind::RefMode
        | FixedU16RecordKind::Iteration
        | FixedU16RecordKind::PrintHeaders
        | FixedU16RecordKind::PrintGridlines
        | FixedU16RecordKind::DefaultColWidth
        | FixedU16RecordKind::Uncalced
        | FixedU16RecordKind::SaveRecalc
        | FixedU16RecordKind::ObjectProtect
        | FixedU16RecordKind::Gridset
        | FixedU16RecordKind::HCenter
        | FixedU16RecordKind::VCenter
        | FixedU16RecordKind::AutoFilterInfo
        | FixedU16RecordKind::Protect
        | FixedU16RecordKind::Password
        | FixedU16RecordKind::ScenarioProtect
        | FixedU16RecordKind::StandardWidth
    ),
    BiffRecordData::Empty { kind, .. } => matches!(
      kind,
      EmptyRecordKind::NullCompatibility | EmptyRecordKind::FilterMode
    ),
    BiffRecordData::ExtendedHeaderFooter(value) => value.sheet_view_guid == [0; 16],
    BiffRecordData::FeatureHeader(value) => value.shared_feature_type == 0x0002,
    BiffRecordData::Feature(value) => matches!(&value.data, FeatureData::Protection(_)),
    _ => false,
  }
}

fn convert_workbook_properties(
  view: &XlsWorkbookView<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<WorkbookProperties>> {
  let location = SourceLocation::XlsWorkbook { workbook_index: 0 };
  let mut date_1904 = Vec::new();
  let mut backup = Vec::new();
  let mut hide_objects = Vec::new();
  let mut refresh_all = Vec::new();
  let mut book_boolean = Vec::new();
  let mut code_names = Vec::new();
  let mut book_extensions = Vec::new();
  let mut compress_pictures = Vec::new();
  let mut compatibility = Vec::new();
  for record in view.globals_records() {
    match &record.data {
      BiffRecordData::FixedU16 { kind, value } => match kind {
        FixedU16RecordKind::Date1904 => date_1904.push(*value),
        FixedU16RecordKind::Backup => backup.push(*value),
        FixedU16RecordKind::HideObj => hide_objects.push(*value),
        FixedU16RecordKind::RefreshAll => refresh_all.push(*value),
        FixedU16RecordKind::BookBool => book_boolean.push(*value),
        _ => {}
      },
      BiffRecordData::CodeName(value) => code_names.push(value),
      BiffRecordData::BookExt(value) => book_extensions.push(value),
      BiffRecordData::CompressPictures(value) => compress_pictures.push(value),
      BiffRecordData::Compat12(value) => compatibility.push(value),
      _ => {}
    }
  }

  let date_1904 = required_single_u16(
    &date_1904,
    ConversionCode::WorkbookPropertiesNotMapped,
    location,
    options,
    report,
  )?;
  let backup = required_single_u16(
    &backup,
    ConversionCode::WorkbookPropertiesNotMapped,
    location,
    options,
    report,
  )?;
  let hide_objects = required_single_u16(
    &hide_objects,
    ConversionCode::WorkbookPropertiesNotMapped,
    location,
    options,
    report,
  )?;
  let refresh_all = required_single_u16(
    &refresh_all,
    ConversionCode::WorkbookPropertiesNotMapped,
    location,
    options,
    report,
  )?;
  let book_boolean = required_single_u16(
    &book_boolean,
    ConversionCode::WorkbookPropertiesNotMapped,
    location,
    options,
    report,
  )?;

  let mut target = WorkbookProperties::default();
  let mut mapped = false;
  if let Some(value) = date_1904
    && let Some(value) = checked_biff_boolean_u16(
      value,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
      options,
      report,
    )?
  {
    target.date1904 = Some(BooleanValue::from_bool(value));
    mapped = true;
  }
  if let Some(value) = backup
    && let Some(value) = checked_biff_boolean_u16(
      value,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
      options,
      report,
    )?
  {
    target.backup_file = Some(BooleanValue::from_bool(value));
    mapped = true;
  }
  if let Some(value) = refresh_all
    && let Some(value) = checked_biff_boolean_u16(
      value,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
      options,
      report,
    )?
  {
    target.refresh_all_connections = Some(BooleanValue::from_bool(value));
    mapped = true;
  }
  if let Some(value) = hide_objects {
    target.show_objects = match value {
      0 => Some(ObjectDisplayValues::All),
      1 => Some(ObjectDisplayValues::Placeholders),
      2 => Some(ObjectDisplayValues::None),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorkbookPropertiesNotMapped,
          location,
        )?;
        None
      }
    };
    mapped |= target.show_objects.is_some();
  }
  if let Some(value) = book_boolean {
    // MS-XLS BookBool: bit 0 is inverted, bits 5..=6 are grUpdateLinks,
    // and bit 8 hides unselected table borders. Bit 7 is undefined and ignored.
    if value & 0xfe02 != 0 || (value >> 5) & 0x3 == 3 || value & 0x001c != 0 {
      unsupported(
        report,
        options,
        ConversionCode::WorkbookPropertiesNotMapped,
        location,
      )?;
    }
    target.save_external_link_values = Some(BooleanValue::from_bool(value & 0x0001 == 0));
    target.update_links = match (value >> 5) & 0x3 {
      0 => Some(UpdateLinksBehaviorValues::UserSet),
      1 => Some(UpdateLinksBehaviorValues::Never),
      2 => Some(UpdateLinksBehaviorValues::Always),
      _ => None,
    };
    target.show_border_unselected_tables = Some(BooleanValue::from_bool(value & 0x0100 == 0));
    mapped = true;
  }

  if code_names.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
    )?;
  }
  if let Some(value) = code_names.first() {
    target.code_name = biff_string_text(&value.name.text);
    if target.code_name.is_none() {
      unsupported(
        report,
        options,
        ConversionCode::CompatibilityUtf16,
        location,
      )?;
    } else {
      mapped = true;
    }
  }

  if book_extensions.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
    )?;
  }
  if let Some(value) = book_extensions.first() {
    use olecfsdk::xls::{BookExtConditional11Flags, BookExtConditional12Flags, BookExtFlags};
    let mapped_flags = BookExtFlags::HIDE_PIVOT_LIST | BookExtFlags::FILTER_PRIVACY;
    if value.flags.bits() & !mapped_flags.bits() != 0 {
      unsupported(
        report,
        options,
        ConversionCode::WorkbookPropertiesNotMapped,
        location,
      )?;
    }
    target.hide_pivot_field_list = Some(BooleanValue::from_bool(
      value.flags.contains(BookExtFlags::HIDE_PIVOT_LIST),
    ));
    target.filter_privacy = Some(BooleanValue::from_bool(
      value.flags.contains(BookExtFlags::FILTER_PRIVACY),
    ));
    if let Some(flags) = value.conditional11 {
      target.prompted_solutions = Some(BooleanValue::from_bool(
        flags.contains(BookExtConditional11Flags::WARN_ABOUT_SOLUTION),
      ));
      target.show_ink_annotation = Some(BooleanValue::from_bool(
        flags.contains(BookExtConditional11Flags::SHOW_INK_ANNOTATION),
      ));
    }
    if let Some(flags) = value.conditional12 {
      target.publish_items = Some(BooleanValue::from_bool(
        flags.contains(BookExtConditional12Flags::PUBLISHED_BOOK_ITEMS),
      ));
      target.show_pivot_chart_filter = Some(BooleanValue::from_bool(
        flags.contains(BookExtConditional12Flags::SHOW_PIVOT_CHART_FILTER),
      ));
    }
    mapped = true;
  }

  if compress_pictures.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
    )?;
  }
  if let Some(value) = compress_pictures.first()
    && let Some(value) = checked_biff_boolean_u32(
      value.auto_compress_pictures,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
      options,
      report,
    )?
  {
    target.auto_compress_pictures = Some(BooleanValue::from_bool(value));
    mapped = true;
  }

  if compatibility.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
    )?;
  }
  if let Some(value) = compatibility.first()
    && let Some(value) = checked_biff_boolean_u32(
      value.no_compatibility_check,
      ConversionCode::WorkbookPropertiesNotMapped,
      location,
      options,
      report,
    )?
  {
    target.check_compatibility = Some(BooleanValue::from_bool(!value));
    mapped = true;
  }

  if mapped {
    report.record(Disposition::Mapped);
    Ok(Some(target))
  } else {
    Ok(None)
  }
}

fn convert_workbook_protection(
  view: &XlsWorkbookView<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<WorkbookProtection>> {
  let location = SourceLocation::XlsWorkbook { workbook_index: 0 };
  let mut windows = Vec::new();
  let mut structure = Vec::new();
  let mut passwords = Vec::new();
  let mut revisions = Vec::new();
  let mut revision_passwords = Vec::new();
  for record in view.globals_records() {
    let BiffRecordData::FixedU16 { kind, value } = record.data else {
      continue;
    };
    match kind {
      FixedU16RecordKind::WindowProtect => windows.push(value),
      FixedU16RecordKind::Protect => structure.push(value),
      FixedU16RecordKind::Password => passwords.push(value),
      FixedU16RecordKind::ProtectionRev4 => revisions.push(value),
      FixedU16RecordKind::PasswordRev4 => revision_passwords.push(value),
      _ => {}
    }
  }

  // MS-XLS Globals PROTECTION requires exactly one record of each kind.
  let windows = required_single_u16(
    &windows,
    ConversionCode::WorkbookProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let structure = required_single_u16(
    &structure,
    ConversionCode::WorkbookProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let password = required_single_u16(
    &passwords,
    ConversionCode::WorkbookProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let revisions = required_single_u16(
    &revisions,
    ConversionCode::WorkbookProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let revision_password = required_single_u16(
    &revision_passwords,
    ConversionCode::WorkbookProtectionNotMapped,
    location,
    options,
    report,
  )?;

  let lock_windows = windows
    .map(|value| {
      checked_biff_boolean_u16(
        value,
        ConversionCode::WorkbookProtectionNotMapped,
        location,
        options,
        report,
      )
    })
    .transpose()?
    .flatten();
  let lock_structure = structure
    .map(|value| {
      checked_biff_boolean_u16(
        value,
        ConversionCode::WorkbookProtectionNotMapped,
        location,
        options,
        report,
      )
    })
    .transpose()?
    .flatten();
  let lock_revision = revisions
    .map(|value| {
      checked_biff_boolean_u16(
        value,
        ConversionCode::WorkbookProtectionNotMapped,
        location,
        options,
        report,
      )
    })
    .transpose()?
    .flatten();
  if lock_revision == Some(false) && revision_password.is_some_and(|value| value != 0) {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookProtectionNotMapped,
      location,
    )?;
  }

  let present = lock_windows == Some(true)
    || lock_structure == Some(true)
    || lock_revision == Some(true)
    || password.is_some_and(|value| value != 0)
    || revision_password.is_some_and(|value| value != 0);
  report.record(Disposition::Mapped);
  Ok(present.then(|| {
    WorkbookProtection {
      workbook_password: password
        .filter(|value| *value != 0)
        .map(legacy_password_hex),
      revisions_password: revision_password
        .filter(|value| *value != 0)
        .map(legacy_password_hex),
      lock_structure: lock_structure.map(BooleanValue::from_bool),
      lock_windows: lock_windows.map(BooleanValue::from_bool),
      lock_revision: lock_revision.map(BooleanValue::from_bool),
      ..Default::default()
    }
  }))
}

fn legacy_password_hex(value: u16) -> String {
  format!("{value:04X}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct XlsCalculationSettings {
  mode: u16,
  iteration_count: u16,
  reference_mode: u16,
  iteration: u16,
  delta_bits: u64,
  calculate_on_save: u16,
}

fn convert_calculation_properties(
  view: &XlsWorkbookView<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<CalculationProperties>> {
  let workbook_location = SourceLocation::XlsWorkbook { workbook_index: 0 };
  let mut precision = Vec::new();
  let mut recalculation_ids = Vec::new();
  let mut force_full_calculation = Vec::new();
  let mut multithreaded = Vec::new();
  for record in view.globals_records() {
    match &record.data {
      BiffRecordData::FixedU16 {
        kind: FixedU16RecordKind::CalcPrecision,
        value,
      } => precision.push(*value),
      BiffRecordData::RecalcId(value) => recalculation_ids.push(value),
      BiffRecordData::ForceFullCalculation(value) => force_full_calculation.push(value),
      BiffRecordData::MtrSettings(value) => multithreaded.push(value),
      _ => {}
    }
  }
  let precision = required_single_u16(
    &precision,
    ConversionCode::WorkbookCalculationNotMapped,
    workbook_location,
    options,
    report,
  )?;

  let mut settings = Vec::new();
  let mut any_uncalculated = false;
  for (sheet_index, sheet) in view.sheets().iter().copied().enumerate() {
    if sheet.kind() != BiffSubstreamKind::WorksheetOrDialogSheet || sheet.metadata().sheet_type != 0
    {
      continue;
    }
    let location = SourceLocation::XlsSheet {
      workbook_index: 0,
      sheet_index,
    };
    if let Some(value) = collect_sheet_calculation_settings(sheet, location, options, report)? {
      settings.push((sheet_index, value));
    }
    any_uncalculated |= sheet.direct_records().any(|record| {
      matches!(
        record.data,
        BiffRecordData::FixedU16 {
          kind: FixedU16RecordKind::Uncalced,
          ..
        }
      )
    });
  }
  let common = settings.first().map(|(_, value)| *value);
  if settings.iter().any(|(_, value)| Some(*value) != common) {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
    )?;
  }

  let mut target = CalculationProperties {
    full_calculation_on_load: Some(BooleanValue::from_bool(any_uncalculated)),
    calculation_completed: Some(BooleanValue::from_bool(!any_uncalculated)),
    ..Default::default()
  };
  if let Some(value) = common {
    target.calculation_mode = match value.mode {
      0 => Some(CalculateModeValues::Manual),
      1 => Some(CalculateModeValues::Auto),
      2 => Some(CalculateModeValues::AutoNoTable),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorkbookCalculationNotMapped,
          workbook_location,
        )?;
        None
      }
    };
    if (1..=32_767).contains(&value.iteration_count) {
      target.iterate_count = Some(u32::from(value.iteration_count));
    } else {
      unsupported(
        report,
        options,
        ConversionCode::WorkbookCalculationNotMapped,
        workbook_location,
      )?;
    }
    target.reference_mode = match value.reference_mode {
      0 => Some(ReferenceModeValues::R1c1),
      1 => Some(ReferenceModeValues::A1),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorkbookCalculationNotMapped,
          workbook_location,
        )?;
        None
      }
    };
    target.iterate = checked_biff_boolean_u16(
      value.iteration,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?
    .map(BooleanValue::from_bool);
    let delta = f64::from_bits(value.delta_bits);
    if delta.is_finite() && delta >= 0.0 {
      target.iterate_delta = Some(delta);
    } else {
      unsupported(
        report,
        options,
        ConversionCode::WorkbookCalculationNotMapped,
        workbook_location,
      )?;
    }
    target.calculation_on_save = checked_biff_boolean_u16(
      value.calculate_on_save,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?
    .map(BooleanValue::from_bool);
  }
  if let Some(value) = precision {
    target.full_precision = checked_biff_boolean_u16(
      value,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?
    .map(BooleanValue::from_bool);
  }

  if recalculation_ids.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
    )?;
  }
  if let Some(value) = recalculation_ids.first() {
    if value.record_type != 449 || value.reserved != 0 {
      unsupported(
        report,
        options,
        ConversionCode::WorkbookCalculationNotMapped,
        workbook_location,
      )?;
    }
    target.calculation_id = Some(value.engine_build);
  }

  if force_full_calculation.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
    )?;
  }
  if let Some(value) = force_full_calculation.first() {
    target.force_full_calculation = checked_biff_boolean_u32(
      value.ignore_dependencies,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?
    .map(BooleanValue::from_bool);
  }

  if multithreaded.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
    )?;
  }
  if let Some(value) = multithreaded.first() {
    let enabled = checked_biff_boolean_u32(
      value.enabled,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?;
    let user_set = checked_biff_boolean_u32(
      value.user_set_thread_count,
      ConversionCode::WorkbookCalculationNotMapped,
      workbook_location,
      options,
      report,
    )?;
    target.concurrent_calculation = enabled.map(BooleanValue::from_bool);
    if enabled == Some(true) && user_set == Some(true) {
      if (1..=1024).contains(&value.thread_count) {
        target.concurrent_manual_count = Some(value.thread_count as u32);
      } else {
        unsupported(
          report,
          options,
          ConversionCode::WorkbookCalculationNotMapped,
          workbook_location,
        )?;
      }
    }
  }

  report.record(Disposition::Mapped);
  Ok(Some(target))
}

fn collect_sheet_calculation_settings(
  source: XlsSheetRef<'_>,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<XlsCalculationSettings>> {
  let mut mode = Vec::new();
  let mut iteration_count = Vec::new();
  let mut reference_mode = Vec::new();
  let mut iteration = Vec::new();
  let mut delta_bits = Vec::new();
  let mut calculate_on_save = Vec::new();
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::FixedU16 { kind, value } => match kind {
        FixedU16RecordKind::CalcMode => mode.push(*value),
        FixedU16RecordKind::CalcCount => iteration_count.push(*value),
        FixedU16RecordKind::RefMode => reference_mode.push(*value),
        FixedU16RecordKind::Iteration => iteration.push(*value),
        FixedU16RecordKind::SaveRecalc => calculate_on_save.push(*value),
        _ => {}
      },
      BiffRecordData::FixedF64Bits {
        kind: FixedF64RecordKind::CalcDelta,
        bits,
      } => delta_bits.push(*bits),
      _ => {}
    }
  }
  let values = [
    mode.len(),
    iteration_count.len(),
    reference_mode.len(),
    iteration.len(),
    delta_bits.len(),
    calculate_on_save.len(),
  ];
  if values != [1; 6] {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookCalculationNotMapped,
      location,
    )?;
  }
  let Some((mode, iteration_count, reference_mode, iteration, delta_bits, calculate_on_save)) =
    mode
      .first()
      .copied()
      .zip(iteration_count.first().copied())
      .zip(reference_mode.first().copied())
      .zip(iteration.first().copied())
      .zip(delta_bits.first().copied())
      .zip(calculate_on_save.first().copied())
      .map(|(((((a, b), c), d), e), f)| (a, b, c, d, e, f))
  else {
    return Ok(None);
  };
  Ok(Some(XlsCalculationSettings {
    mode,
    iteration_count,
    reference_mode,
    iteration,
    delta_bits,
    calculate_on_save,
  }))
}

fn convert_sheet_calculation_properties(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SheetCalculationProperties>> {
  let values = source
    .direct_records()
    .filter_map(|record| match record.data {
      BiffRecordData::FixedU16 {
        kind: FixedU16RecordKind::Uncalced,
        value,
      } => Some(value),
      _ => None,
    })
    .collect::<Vec<_>>();
  if values.is_empty() {
    return Ok(None);
  }
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  if values.len() != 1 || values[0] != 0 {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetCalculationNotMapped,
      location,
    )?;
  }
  report.record(Disposition::Mapped);
  Ok(Some(SheetCalculationProperties {
    full_calculation_on_load: Some(BooleanValue::from_bool(true)),
  }))
}

fn convert_sheet_protection(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SheetProtection>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut protect = Vec::new();
  let mut passwords = Vec::new();
  let mut objects = Vec::new();
  let mut scenarios = Vec::new();
  let mut enhanced = Vec::new();
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::FixedU16 { kind, value } => match kind {
        FixedU16RecordKind::Protect => protect.push(*value),
        FixedU16RecordKind::Password => passwords.push(*value),
        FixedU16RecordKind::ObjectProtect => objects.push(*value),
        FixedU16RecordKind::ScenarioProtect => scenarios.push(*value),
        _ => {}
      },
      BiffRecordData::FeatureHeader(value) if value.shared_feature_type == 0x0002 => {
        if value.header.record_type != 0x0867
          || !value.header.flags.is_empty()
          || value.header.reserved != 0
          || value.reserved != 1
        {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetProtectionNotMapped,
            location,
          )?;
        }
        match value.data {
          FeatureHeaderData::EnhancedProtection(flags) => {
            if flags.bits() & !EnhancedProtectionFlags::all().bits() != 0 {
              unsupported(
                report,
                options,
                ConversionCode::WorksheetProtectionNotMapped,
                location,
              )?;
            }
            enhanced.push(flags);
          }
          FeatureHeaderData::None => {}
          _ => {
            unsupported(
              report,
              options,
              ConversionCode::WorksheetProtectionNotMapped,
              location,
            )?;
          }
        }
      }
      _ => {}
    }
  }

  let has_classic_records =
    !protect.is_empty() || !passwords.is_empty() || !objects.is_empty() || !scenarios.is_empty();
  if !has_classic_records {
    return Ok(None);
  }
  let protect = optional_single_u16(
    &protect,
    ConversionCode::WorksheetProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let password = optional_single_u16(
    &passwords,
    ConversionCode::WorksheetProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let object = optional_single_u16(
    &objects,
    ConversionCode::WorksheetProtectionNotMapped,
    location,
    options,
    report,
  )?;
  let scenario = optional_single_u16(
    &scenarios,
    ConversionCode::WorksheetProtectionNotMapped,
    location,
    options,
    report,
  )?;
  if protect.is_some_and(|value| value != 1)
    || password == Some(0)
    || object.is_some_and(|value| value != 1)
    || scenario.is_some_and(|value| value != 1)
    || (protect.is_none() && (password.is_some() || object.is_some() || scenario.is_some()))
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetProtectionNotMapped,
      location,
    )?;
  }
  if enhanced.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetProtectionNotMapped,
      location,
    )?;
  }

  let mut target = SheetProtection {
    password: password
      .filter(|value| *value != 0)
      .map(legacy_password_hex),
    sheet: Some(BooleanValue::from_bool(protect.is_some())),
    objects: Some(BooleanValue::from_bool(object.is_some())),
    scenarios: Some(BooleanValue::from_bool(scenario.is_some())),
    ..Default::default()
  };
  if let Some(flags) = enhanced.first().copied() {
    target.objects = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::OBJECTS),
    ));
    target.scenarios = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::SCENARIOS),
    ));
    target.format_cells = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::FORMAT_CELLS),
    ));
    target.format_columns = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::FORMAT_COLUMNS),
    ));
    target.format_rows = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::FORMAT_ROWS),
    ));
    target.insert_columns = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::INSERT_COLUMNS),
    ));
    target.insert_rows = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::INSERT_ROWS),
    ));
    target.insert_hyperlinks = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::INSERT_HYPERLINKS),
    ));
    target.delete_columns = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::DELETE_COLUMNS),
    ));
    target.delete_rows = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::DELETE_ROWS),
    ));
    target.select_locked_cells = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::SELECT_LOCKED_CELLS),
    ));
    target.sort = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::SORT),
    ));
    target.auto_filter = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::AUTO_FILTER),
    ));
    target.pivot_tables = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::PIVOT_TABLES),
    ));
    target.select_unlocked_cells = Some(BooleanValue::from_bool(
      !flags.contains(EnhancedProtectionFlags::SELECT_UNLOCKED_CELLS),
    ));
  }
  report.record(Disposition::Mapped);
  Ok(Some(target))
}

fn convert_protected_ranges(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<ProtectedRanges>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut ranges = Vec::new();
  for record in source.direct_records() {
    let BiffRecordData::Feature(feature) = &record.data else {
      continue;
    };
    let FeatureData::Protection(protection) = &feature.data else {
      continue;
    };
    if feature.header.record_type != 0x0868
      || !feature.header.flags.is_empty()
      || feature.header.reserved != 0
      || feature.shared_feature_type != 0x0002
      || feature.reserved1 != 0
      || feature.reserved2 != 0
      || feature.feature_data_size != 0
      || feature.reserved3 != 0
      || protection.flags.bits() & 0xFFFF_FFFE != 0
      || feature.references.is_empty()
      || feature.references.iter().any(|range| {
        range.first_row > range.last_row
          || range.first_column > range.last_column
          || range.last_column > 0x00FF
      })
    {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetProtectedRangeNotMapped,
        location,
      )?;
    }
    if protection.security_descriptor.is_some() {
      // BIFF stores a self-relative binary descriptor; OOXML's transitional
      // string attribute has no normative binary representation.
      unsupported(
        report,
        options,
        ConversionCode::WorksheetProtectedRangeNotMapped,
        location,
      )?;
    }
    let Some(name) = biff_string_text(&protection.title.text) else {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetProtectedRangeNotMapped,
        location,
      )?;
      continue;
    };
    let password = match u16::try_from(protection.password_verifier) {
      Ok(0) => None,
      Ok(value) => Some(legacy_password_hex(value)),
      Err(_) => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetProtectedRangeNotMapped,
          location,
        )?;
        None
      }
    };
    ranges.push(ProtectedRange {
      password,
      sequence_of_references: feature
        .references
        .iter()
        .map(|range| {
          cell_range_reference(
            range.first_row,
            range.first_column,
            range.last_row,
            range.last_column,
          )
        })
        .collect(),
      name,
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  Ok((!ranges.is_empty()).then_some(ProtectedRanges {
    protected_range: ranges,
  }))
}

fn required_single_u16(
  values: &[u16],
  code: ConversionCode,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<u16>> {
  if values.len() != 1 {
    unsupported(report, options, code, location)?;
  }
  Ok(values.first().copied())
}

fn optional_single_u16(
  values: &[u16],
  code: ConversionCode,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<u16>> {
  if values.len() > 1 {
    unsupported(report, options, code, location)?;
  }
  Ok(values.first().copied())
}

fn checked_biff_boolean_u16(
  value: u16,
  code: ConversionCode,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<bool>> {
  checked_biff_boolean_u32(u32::from(value), code, location, options, report)
}

fn checked_biff_boolean_u32(
  value: u32,
  code: ConversionCode,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<bool>> {
  match value {
    0 => Ok(Some(false)),
    1 => Ok(Some(true)),
    _ => {
      unsupported(report, options, code, location)?;
      Ok(None)
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct XlsFilterRange {
  first_row: u32,
  last_row: u32,
  first_column: u32,
  last_column: u32,
}

impl XlsFilterRange {
  fn from_rfx(value: Rfx) -> Self {
    Self {
      first_row: value.first_row,
      last_row: value.last_row,
      first_column: value.first_column,
      last_column: value.last_column,
    }
  }

  fn reference(self) -> Option<String> {
    cell_range_reference_u32(
      self.first_row,
      self.first_column,
      self.last_row,
      self.last_column,
    )
  }

  fn column_count(self) -> Option<u32> {
    self
      .last_column
      .checked_sub(self.first_column)?
      .checked_add(1)
  }
}

#[derive(Default)]
struct ConvertedSheetSortAndFilter {
  auto_filter: Option<Box<x::AutoFilter>>,
  sort_state: Option<Box<x::SortState>>,
}

fn convert_sheet_sort_and_filter(
  view: &XlsWorkbookView<'_>,
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<ConvertedSheetSortAndFilter> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut filter_info = Vec::new();
  let mut classic_filters = Vec::new();
  let mut future_filters = Vec::new();
  let mut sort_data = Vec::new();
  let mut filter_mode_count = 0usize;
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::FixedU16 {
        kind: FixedU16RecordKind::AutoFilterInfo,
        value,
      } => filter_info.push(*value),
      BiffRecordData::AutoFilter(value) => classic_filters.push(value),
      BiffRecordData::AutoFilter12(value) => future_filters.push(value),
      BiffRecordData::SortData(value) => sort_data.push(value),
      BiffRecordData::Empty {
        kind: EmptyRecordKind::FilterMode,
        ..
      } => filter_mode_count += 1,
      _ => {}
    }
  }

  let name_range = filter_database_range(view, sheet_index, location, options, report)?;
  let mut main_future_filters = Vec::new();
  for filter in future_filters {
    if !filter.flags.worksheet || filter.list_id != u32::MAX || filter.user_view_guid != [0; 16] {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetAutoFilterNotMapped,
        location,
      )?;
      continue;
    }
    main_future_filters.push(filter);
  }
  let future_range = main_future_filters.first().map(|filter| {
    let range = filter.header.range;
    XlsFilterRange {
      first_row: u32::from(range.first_row),
      last_row: u32::from(range.last_row),
      first_column: u32::from(range.first_column),
      last_column: u32::from(range.last_column),
    }
  });
  if let Some(range) = future_range
    && main_future_filters.iter().any(|filter| {
      let candidate = filter.header.range;
      range
        != XlsFilterRange {
          first_row: u32::from(candidate.first_row),
          last_row: u32::from(candidate.last_row),
          first_column: u32::from(candidate.first_column),
          last_column: u32::from(candidate.last_column),
        }
    })
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }
  if name_range.is_some() && future_range.is_some() && name_range != future_range {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }
  let filter_range = name_range.or(future_range);
  let has_filter_records = !classic_filters.is_empty() || !main_future_filters.is_empty();
  let has_filter_feature = !filter_info.is_empty() || has_filter_records || filter_mode_count != 0;
  if filter_info.len() > 1
    || filter_mode_count > 1
    || (has_filter_records && filter_info.len() != 1)
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }
  if let (Some(range), Some(entries)) = (filter_range, filter_info.first().copied())
    && (!(1..=256).contains(&entries) || range.column_count() != Some(u32::from(entries)))
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }

  let mut auto_filter = if has_filter_feature {
    match filter_range.and_then(XlsFilterRange::reference) {
      Some(reference) => Some(x::AutoFilter {
        reference: Some(reference),
        ..Default::default()
      }),
      None => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?;
        None
      }
    }
  } else {
    None
  };

  if let Some(target) = &mut auto_filter {
    let entry_limit = filter_info.first().copied();
    for filter in classic_filters {
      if entry_limit.is_some_and(|count| filter.entry_index >= count) {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?;
        continue;
      }
      let column = convert_classic_filter_column(filter, location, options, report)?;
      push_unique_filter_column(target, column, location, options, report)?;
    }
    for filter in main_future_filters {
      if entry_limit.is_some_and(|count| filter.entry_index >= count) {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?;
        continue;
      }
      let column = convert_future_filter_column(filter, location, options, report)?;
      push_unique_filter_column(target, column, location, options, report)?;
    }
    target.filter_column.sort_by_key(|column| column.column_id);
    report.record(Disposition::Mapped);
  }

  let mut worksheet_sort = None;
  let mut filter_sort = None;
  for source_sort in sort_data {
    let target = convert_sort_state(source_sort, location, options, report)?;
    match source_sort.options.parent {
      SortFieldParent::Sheet => {
        if worksheet_sort.is_some() {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetSortStateNotMapped,
            location,
          )?;
        } else {
          worksheet_sort = target.map(Box::new);
        }
      }
      SortFieldParent::AutoFilter => {
        if filter_sort.is_some() {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetSortStateNotMapped,
            location,
          )?;
        } else {
          filter_sort = target.map(Box::new);
        }
      }
      SortFieldParent::Table | SortFieldParent::QueryTable => unsupported(
        report,
        options,
        ConversionCode::WorksheetSortStateNotMapped,
        location,
      )?,
    }
  }
  if let Some(filter_sort) = filter_sort {
    match &mut auto_filter {
      Some(auto_filter) => auto_filter.sort_state = Some(filter_sort),
      None => unsupported(
        report,
        options,
        ConversionCode::WorksheetSortStateNotMapped,
        location,
      )?,
    }
  }
  Ok(ConvertedSheetSortAndFilter {
    auto_filter: auto_filter.map(Box::new),
    sort_state: worksheet_sort,
  })
}

fn filter_database_range(
  view: &XlsWorkbookView<'_>,
  sheet_index: usize,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<XlsFilterRange>> {
  let expected_scope = u16::try_from(sheet_index + 1)
    .map_err(|_| olecfsdk::Error::Limit("XLS sheet index exceeds u16".into()))?;
  let names = view
    .defined_names()
    .iter()
    .copied()
    .filter(|name| name.sheet_index == expected_scope && name.name == NameValue::BuiltIn(0x0d))
    .collect::<Vec<_>>();
  if names.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }
  let Some(name) = names.first().copied() else {
    return Ok(None);
  };
  let valid_envelope = name.formula.tokens.len() == 1
    && name.formula.unparsed_tail.is_empty()
    && name.formula_extra_tail.is_empty();
  let Some(token) = name.formula.tokens.first().filter(|_| valid_envelope) else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let FormulaTokenData::Area3d {
    first_row,
    last_row,
    first_column,
    last_column,
    ..
  } = token.data
  else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let external_sheet = view.resolve_formula_token_external_sheet(&token.data)?;
  let expected_sheet = u16::try_from(sheet_index)
    .map_err(|_| olecfsdk::Error::Limit("XLS sheet index exceeds u16".into()))?;
  if !external_sheet.is_some_and(|sheet| {
    sheet.source().first_sheet_index == expected_sheet
      && sheet.source().last_sheet_index == expected_sheet
  }) {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
    return Ok(None);
  }
  Ok(Some(XlsFilterRange {
    first_row: u32::from(first_row),
    last_row: u32::from(last_row),
    first_column: u32::from(first_column),
    last_column: u32::from(last_column),
  }))
}

fn push_unique_filter_column(
  target: &mut x::AutoFilter,
  column: x::FilterColumn,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<()> {
  if target
    .filter_column
    .iter()
    .any(|existing| existing.column_id == column.column_id)
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  } else {
    target.filter_column.push(column);
  }
  Ok(())
}

fn convert_classic_filter_column(
  source: &AutoFilterRecord,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<x::FilterColumn> {
  let choice = if source.options.top_n {
    if !(1..=500).contains(&source.options.top_count) {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetAutoFilterNotMapped,
        location,
      )?;
    }
    Some(x::FilterColumnChoice::Top10(x::Top10 {
      top: Some(BooleanValue::from_bool(source.options.top)),
      percent: Some(BooleanValue::from_bool(source.options.percent)),
      val: f64::from(source.options.top_count),
      ..Default::default()
    }))
  } else {
    let mut filters = Vec::new();
    for operand in &source.operands {
      match convert_custom_filter_operand(operand) {
        Some(filter) => filters.push(filter),
        None if !matches!(operand.value, AutoFilterOperandValue::Unused { .. }) => unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?,
        None => {}
      }
    }
    (!filters.is_empty()).then(|| {
      x::FilterColumnChoice::XCustomFilters(x::CustomFilters {
        and: (filters.len() > 1).then(|| BooleanValue::from_bool(!source.options.join_or)),
        custom_filter: filters,
      })
    })
  };
  Ok(x::FilterColumn {
    column_id: u32::from(source.entry_index),
    filter_column_choice: choice,
    ..Default::default()
  })
}

fn convert_custom_filter_operand(source: &AutoFilterOperand) -> Option<x::CustomFilter> {
  let (operator, value) = match source.value {
    AutoFilterOperandValue::Unused { .. } => return None,
    AutoFilterOperandValue::Blanks { .. } => (x::FilterOperatorValues::Equal, String::new()),
    AutoFilterOperandValue::NonBlanks { .. } => (x::FilterOperatorValues::NotEqual, String::new()),
    _ => (
      convert_filter_operator(source.comparison)?,
      filter_operand_text(source)?,
    ),
  };
  Some(x::CustomFilter {
    operator: Some(operator),
    val: Some(value),
  })
}

fn convert_filter_operator(value: u8) -> Option<x::FilterOperatorValues> {
  Some(match value {
    1 => x::FilterOperatorValues::LessThan,
    2 => x::FilterOperatorValues::Equal,
    3 => x::FilterOperatorValues::LessThanOrEqual,
    4 => x::FilterOperatorValues::GreaterThan,
    5 => x::FilterOperatorValues::NotEqual,
    6 => x::FilterOperatorValues::GreaterThanOrEqual,
    _ => return None,
  })
}

fn filter_operand_text(source: &AutoFilterOperand) -> Option<String> {
  match source.value {
    AutoFilterOperandValue::Unused { .. }
    | AutoFilterOperandValue::Blanks { .. }
    | AutoFilterOperandValue::NonBlanks { .. } => None,
    AutoFilterOperandValue::Rk { value, .. } => Some(decode_filter_rk(value).to_string()),
    AutoFilterOperandValue::Number { bits } => {
      let value = f64::from_bits(bits);
      value.is_finite().then(|| value.to_string())
    }
    AutoFilterOperandValue::String { .. } => source.string.as_ref().and_then(biff_string_text),
    AutoFilterOperandValue::BooleanOrError { value, .. } => {
      let [raw, error] = value.to_le_bytes();
      match (error, raw) {
        (0, 0 | 1) => Some(raw.to_string()),
        (1, raw) => formula_error(raw).map(str::to_owned),
        _ => None,
      }
    }
  }
}

fn decode_filter_rk(bits: u32) -> f64 {
  let mut value = if bits & 2 != 0 {
    f64::from((bits as i32) >> 2)
  } else {
    f64::from_bits(u64::from(bits & !3) << 32)
  };
  if bits & 1 != 0 {
    value /= 100.0;
  }
  value
}

fn convert_future_filter_column(
  source: &AutoFilter12Record,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<x::FilterColumn> {
  let choice = match &source.filter {
    AutoFilter12Filter::Criteria
      if source.dynamic_filter_type != AutoFilter12DynamicFilter::None =>
    {
      Some(x::FilterColumnChoice::DynamicFilter(
        convert_future_dynamic_filter(source, location, options, report)?,
      ))
    }
    AutoFilter12Filter::Criteria => convert_future_criteria(source, location, options, report)?,
    AutoFilter12Filter::CellColor(_) | AutoFilter12Filter::FontColor(_) => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetAutoFilterNotMapped,
        location,
      )?;
      None
    }
    AutoFilter12Filter::Icon {
      icon_set,
      icon_index,
    } => match convert_icon_set(*icon_set) {
      Some(icon_set) => Some(x::FilterColumnChoice::XIconFilter(x::IconFilter {
        icon_set,
        icon_id: Some(*icon_index),
      })),
      None => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?;
        None
      }
    },
  };
  report.record(Disposition::Mapped);
  Ok(x::FilterColumn {
    column_id: u32::from(source.entry_index),
    hidden_button: Some(BooleanValue::from_bool(source.hide_arrow)),
    filter_column_choice: choice,
    ..Default::default()
  })
}

fn convert_future_dynamic_filter(
  source: &AutoFilter12Record,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<x::DynamicFilter> {
  let mut target = x::DynamicFilter {
    r#type: convert_dynamic_filter(source.dynamic_filter_type),
    ..Default::default()
  };
  let active = source
    .criteria
    .iter()
    .filter_map(|criterion| match criterion.value.operand.value {
      AutoFilterOperandValue::Unused { .. } => None,
      AutoFilterOperandValue::Number { bits } => {
        Some((criterion.value.operand.comparison, f64::from_bits(bits)))
      }
      _ => Some((criterion.value.operand.comparison, f64::NAN)),
    })
    .collect::<Vec<_>>();

  let mapped = match source.dynamic_filter_type {
    AutoFilter12DynamicFilter::AboveAverage => match active.as_slice() {
      [(4, value)] if value.is_finite() => {
        target.val = Some(*value);
        true
      }
      _ => false,
    },
    AutoFilter12DynamicFilter::BelowAverage => match active.as_slice() {
      [(1, value)] if value.is_finite() => {
        target.val = Some(*value);
        true
      }
      _ => false,
    },
    AutoFilter12DynamicFilter::Tomorrow
    | AutoFilter12DynamicFilter::Today
    | AutoFilter12DynamicFilter::Yesterday
    | AutoFilter12DynamicFilter::NextWeek
    | AutoFilter12DynamicFilter::ThisWeek
    | AutoFilter12DynamicFilter::LastWeek
    | AutoFilter12DynamicFilter::NextMonth
    | AutoFilter12DynamicFilter::ThisMonth
    | AutoFilter12DynamicFilter::LastMonth
    | AutoFilter12DynamicFilter::NextQuarter
    | AutoFilter12DynamicFilter::ThisQuarter
    | AutoFilter12DynamicFilter::LastQuarter
    | AutoFilter12DynamicFilter::NextYear
    | AutoFilter12DynamicFilter::ThisYear
    | AutoFilter12DynamicFilter::LastYear
    | AutoFilter12DynamicFilter::YearToDate => {
      let minimum = active
        .iter()
        .filter(|(comparison, _)| *comparison == 6)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>();
      let maximum = active
        .iter()
        .filter(|(comparison, _)| *comparison == 1)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>();
      match (active.len(), minimum.as_slice(), maximum.as_slice()) {
        (2, [minimum], [maximum])
          if minimum.is_finite() && maximum.is_finite() && minimum < maximum =>
        {
          target.val = Some(*minimum);
          target.max_val = Some(*maximum);
          true
        }
        _ => false,
      }
    }
    AutoFilter12DynamicFilter::Quarter1
    | AutoFilter12DynamicFilter::Quarter2
    | AutoFilter12DynamicFilter::Quarter3
    | AutoFilter12DynamicFilter::Quarter4
    | AutoFilter12DynamicFilter::Month1
    | AutoFilter12DynamicFilter::Month2
    | AutoFilter12DynamicFilter::Month3
    | AutoFilter12DynamicFilter::Month4
    | AutoFilter12DynamicFilter::Month5
    | AutoFilter12DynamicFilter::Month6
    | AutoFilter12DynamicFilter::Month7
    | AutoFilter12DynamicFilter::Month8
    | AutoFilter12DynamicFilter::Month9
    | AutoFilter12DynamicFilter::Month10
    | AutoFilter12DynamicFilter::Month11
    | AutoFilter12DynamicFilter::Month12 => true,
    AutoFilter12DynamicFilter::None => false,
  };
  if !mapped {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
  }
  Ok(target)
}

fn convert_future_criteria(
  source: &AutoFilter12Record,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<x::FilterColumnChoice>> {
  let equality_list = source.criteria.iter().all(|criterion| {
    criterion.value.operand.comparison == 2
      && !matches!(
        criterion.value.operand.value,
        AutoFilterOperandValue::NonBlanks { .. }
      )
  });
  if equality_list || !source.date_groupings.is_empty() {
    let mut blank = false;
    let mut filters_choice = Vec::new();
    for criterion in &source.criteria {
      let operand = &criterion.value.operand;
      match operand.value {
        AutoFilterOperandValue::Blanks { .. } if operand.comparison == 2 => blank = true,
        AutoFilterOperandValue::Unused { .. } => {}
        _ if operand.comparison == 2 => match filter_operand_text(operand) {
          Some(value) => filters_choice.push(x::FiltersChoice::XFilter(x::Filter { val: value })),
          None => unsupported(
            report,
            options,
            ConversionCode::WorksheetAutoFilterNotMapped,
            location,
          )?,
        },
        _ => unsupported(
          report,
          options,
          ConversionCode::WorksheetAutoFilterNotMapped,
          location,
        )?,
      }
    }
    filters_choice.extend(
      source
        .date_groupings
        .iter()
        .map(|group| x::FiltersChoice::DateGroupItem(convert_date_grouping(&group.value))),
    );
    return Ok(Some(x::FilterColumnChoice::Filters(x::Filters {
      blank: blank.then(|| BooleanValue::from_bool(true)),
      filters_choice,
      ..Default::default()
    })));
  }
  let active = source
    .criteria
    .iter()
    .filter(|criterion| {
      !matches!(
        criterion.value.operand.value,
        AutoFilterOperandValue::Unused { .. }
      )
    })
    .collect::<Vec<_>>();
  if active.len() > 1 {
    // MS-XLS does not carry the OOXML customFilters `and` bit in this
    // production, so do not invent a relationship between two comparisons.
    unsupported(
      report,
      options,
      ConversionCode::WorksheetAutoFilterNotMapped,
      location,
    )?;
    return Ok(None);
  }
  let Some(filter) = active
    .first()
    .and_then(|criterion| convert_custom_filter_operand(&criterion.value.operand))
  else {
    return Ok(None);
  };
  Ok(Some(x::FilterColumnChoice::XCustomFilters(
    x::CustomFilters {
      and: None,
      custom_filter: vec![filter],
    },
  )))
}

fn convert_date_grouping(source: &AutoFilter12DateGrouping) -> x::DateGroupItem {
  let level = source.level;
  x::DateGroupItem {
    year: source.year,
    month: (level != AutoFilter12DateGroupingLevel::Year).then_some(source.month),
    day: matches!(
      level,
      AutoFilter12DateGroupingLevel::Day
        | AutoFilter12DateGroupingLevel::Hour
        | AutoFilter12DateGroupingLevel::Minute
        | AutoFilter12DateGroupingLevel::Second
    )
    .then_some(source.day as u16),
    hour: matches!(
      level,
      AutoFilter12DateGroupingLevel::Hour
        | AutoFilter12DateGroupingLevel::Minute
        | AutoFilter12DateGroupingLevel::Second
    )
    .then_some(source.hour),
    minute: matches!(
      level,
      AutoFilter12DateGroupingLevel::Minute | AutoFilter12DateGroupingLevel::Second
    )
    .then_some(source.minute),
    second: (level == AutoFilter12DateGroupingLevel::Second).then_some(source.second),
    date_time_grouping: match level {
      AutoFilter12DateGroupingLevel::Year => x::DateTimeGroupingValues::Year,
      AutoFilter12DateGroupingLevel::Month => x::DateTimeGroupingValues::Month,
      AutoFilter12DateGroupingLevel::Day => x::DateTimeGroupingValues::Day,
      AutoFilter12DateGroupingLevel::Hour => x::DateTimeGroupingValues::Hour,
      AutoFilter12DateGroupingLevel::Minute => x::DateTimeGroupingValues::Minute,
      AutoFilter12DateGroupingLevel::Second => x::DateTimeGroupingValues::Second,
    },
  }
}

fn convert_dynamic_filter(source: AutoFilter12DynamicFilter) -> x::DynamicFilterValues {
  match source {
    AutoFilter12DynamicFilter::None => x::DynamicFilterValues::Null,
    AutoFilter12DynamicFilter::AboveAverage => x::DynamicFilterValues::AboveAverage,
    AutoFilter12DynamicFilter::BelowAverage => x::DynamicFilterValues::BelowAverage,
    AutoFilter12DynamicFilter::Tomorrow => x::DynamicFilterValues::Tomorrow,
    AutoFilter12DynamicFilter::Today => x::DynamicFilterValues::Today,
    AutoFilter12DynamicFilter::Yesterday => x::DynamicFilterValues::Yesterday,
    AutoFilter12DynamicFilter::NextWeek => x::DynamicFilterValues::NextWeek,
    AutoFilter12DynamicFilter::ThisWeek => x::DynamicFilterValues::ThisWeek,
    AutoFilter12DynamicFilter::LastWeek => x::DynamicFilterValues::LastWeek,
    AutoFilter12DynamicFilter::NextMonth => x::DynamicFilterValues::NextMonth,
    AutoFilter12DynamicFilter::ThisMonth => x::DynamicFilterValues::ThisMonth,
    AutoFilter12DynamicFilter::LastMonth => x::DynamicFilterValues::LastMonth,
    AutoFilter12DynamicFilter::NextQuarter => x::DynamicFilterValues::NextQuarter,
    AutoFilter12DynamicFilter::ThisQuarter => x::DynamicFilterValues::ThisQuarter,
    AutoFilter12DynamicFilter::LastQuarter => x::DynamicFilterValues::LastQuarter,
    AutoFilter12DynamicFilter::NextYear => x::DynamicFilterValues::NextYear,
    AutoFilter12DynamicFilter::ThisYear => x::DynamicFilterValues::ThisYear,
    AutoFilter12DynamicFilter::LastYear => x::DynamicFilterValues::LastYear,
    AutoFilter12DynamicFilter::YearToDate => x::DynamicFilterValues::YearToDate,
    AutoFilter12DynamicFilter::Quarter1 => x::DynamicFilterValues::Quarter1,
    AutoFilter12DynamicFilter::Quarter2 => x::DynamicFilterValues::Quarter2,
    AutoFilter12DynamicFilter::Quarter3 => x::DynamicFilterValues::Quarter3,
    AutoFilter12DynamicFilter::Quarter4 => x::DynamicFilterValues::Quarter4,
    AutoFilter12DynamicFilter::Month1 => x::DynamicFilterValues::January,
    AutoFilter12DynamicFilter::Month2 => x::DynamicFilterValues::February,
    AutoFilter12DynamicFilter::Month3 => x::DynamicFilterValues::March,
    AutoFilter12DynamicFilter::Month4 => x::DynamicFilterValues::April,
    AutoFilter12DynamicFilter::Month5 => x::DynamicFilterValues::May,
    AutoFilter12DynamicFilter::Month6 => x::DynamicFilterValues::June,
    AutoFilter12DynamicFilter::Month7 => x::DynamicFilterValues::July,
    AutoFilter12DynamicFilter::Month8 => x::DynamicFilterValues::August,
    AutoFilter12DynamicFilter::Month9 => x::DynamicFilterValues::September,
    AutoFilter12DynamicFilter::Month10 => x::DynamicFilterValues::October,
    AutoFilter12DynamicFilter::Month11 => x::DynamicFilterValues::November,
    AutoFilter12DynamicFilter::Month12 => x::DynamicFilterValues::December,
  }
}

fn convert_icon_set(source: KpiSet) -> Option<x::IconSetValues> {
  Some(match source {
    KpiSet::None => return None,
    KpiSet::ThreeArrows => x::IconSetValues::ThreeArrows,
    KpiSet::ThreeArrowsGray => x::IconSetValues::ThreeArrowsGray,
    KpiSet::ThreeFlags => x::IconSetValues::ThreeFlags,
    KpiSet::ThreeTrafficLights1 => x::IconSetValues::ThreeTrafficLights1,
    KpiSet::ThreeTrafficLights2 => x::IconSetValues::ThreeTrafficLights2,
    KpiSet::ThreeSigns => x::IconSetValues::ThreeSigns,
    KpiSet::ThreeSymbols => x::IconSetValues::ThreeSymbols,
    KpiSet::ThreeSymbols2 => x::IconSetValues::ThreeSymbols2,
    KpiSet::FourArrows => x::IconSetValues::FourArrows,
    KpiSet::FourArrowsGray => x::IconSetValues::FourArrowsGray,
    KpiSet::FourRedToBlack => x::IconSetValues::FourRedToBlack,
    KpiSet::FourRating => x::IconSetValues::FourRating,
    KpiSet::FourTrafficLights => x::IconSetValues::FourTrafficLights,
    KpiSet::FiveArrows => x::IconSetValues::FiveArrows,
    KpiSet::FiveArrowsGray => x::IconSetValues::FiveArrowsGray,
    KpiSet::FiveRating => x::IconSetValues::FiveRating,
    KpiSet::FiveQuarters => x::IconSetValues::FiveQuarters,
  })
}

fn convert_sort_state(
  source: &SortDataRecord,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<x::SortState>> {
  let Some(reference) = XlsFilterRange::from_rfx(source.range).reference() else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetSortStateNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let sort_method = if source.options.alternate_method {
    // BIFF only says "other than character order"; OOXML requires choosing
    // stroke or phonetic order, which cannot be recovered from this bit.
    unsupported(
      report,
      options,
      ConversionCode::WorksheetSortStateNotMapped,
      location,
    )?;
    None
  } else {
    None
  };
  let mut conditions = Vec::new();
  for continuation in &source.conditions {
    let condition = &continuation.condition;
    let Some(reference) = XlsFilterRange::from_rfx(condition.range).reference() else {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetSortStateNotMapped,
        location,
      )?;
      continue;
    };
    if condition.reserved != 0 {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetSortStateNotMapped,
        location,
      )?;
    }
    let custom_list = condition.custom_list.as_ref().and_then(biff_string_text);
    if condition.custom_list.is_some() && custom_list.is_none() {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetSortStateNotMapped,
        location,
      )?;
    }
    let mut target = x::SortCondition {
      descending: Some(BooleanValue::from_bool(condition.descending)),
      reference,
      custom_list,
      ..Default::default()
    };
    match condition.data {
      SortConditionData::Value { value, reserved } => {
        if value != 0 || reserved != 0 {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetSortStateNotMapped,
            location,
          )?;
        }
        target.sort_by = Some(x::SortByValues::Value);
      }
      SortConditionData::CellColor { .. } | SortConditionData::FontColor { .. } => {
        // SortData refers to Globals DXF indices; those must be emitted into
        // the target stylesheet before a dxfId can be valid.
        unsupported(
          report,
          options,
          ConversionCode::WorksheetSortStateNotMapped,
          location,
        )?;
        continue;
      }
      SortConditionData::Icon {
        icon_set,
        icon_index,
      } => {
        let Some(icon_set) = KpiSet::from_raw(icon_set).and_then(convert_icon_set) else {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetSortStateNotMapped,
            location,
          )?;
          continue;
        };
        target.sort_by = Some(x::SortByValues::Icon);
        target.icon_set = Some(icon_set);
        target.icon_id = u32::try_from(icon_index).ok();
      }
    }
    conditions.push(x::SortStateChoice::XSortCondition(Box::new(target)));
  }
  report.record(Disposition::Mapped);
  Ok(Some(x::SortState {
    column_sort: Some(BooleanValue::from_bool(source.options.sort_columns)),
    case_sensitive: Some(BooleanValue::from_bool(source.options.case_sensitive)),
    sort_method,
    reference,
    sort_state_choice: conditions,
    ..Default::default()
  }))
}

#[derive(Debug)]
struct XlsWindowGroup<'a> {
  window: &'a Window2Record,
  page_layout: Vec<&'a PlvRecord>,
  scale: Vec<&'a SclRecord>,
  pane: Vec<&'a PaneRecord>,
  selections: Vec<&'a SelectionRecord>,
}

#[derive(Debug, Default)]
struct XlsSheetWindows<'a> {
  groups: Vec<XlsWindowGroup<'a>>,
  orphan_view_records: bool,
}

fn collect_sheet_window_groups(source: XlsSheetRef<'_>) -> XlsSheetWindows<'_> {
  let mut result = XlsSheetWindows::default();
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::Window2(window) => result.groups.push(XlsWindowGroup {
        window,
        page_layout: Vec::new(),
        scale: Vec::new(),
        pane: Vec::new(),
        selections: Vec::new(),
      }),
      // WORKSHEETCONTENT puts all normal WINDOW productions before every
      // CUSTOMVIEW. Selection records after this delimiter belong to a
      // custom view and must not be folded into the last normal window.
      BiffRecordData::UserSViewBegin(_) | BiffRecordData::UserSViewBeginChart(_) => break,
      BiffRecordData::Plv(value) => {
        if let Some(group) = result.groups.last_mut() {
          group.page_layout.push(value);
        } else {
          result.orphan_view_records = true;
        }
      }
      BiffRecordData::Scl(value) => {
        if let Some(group) = result.groups.last_mut() {
          group.scale.push(value);
        } else {
          result.orphan_view_records = true;
        }
      }
      BiffRecordData::Pane(value) => {
        if let Some(group) = result.groups.last_mut() {
          group.pane.push(value);
        } else {
          result.orphan_view_records = true;
        }
      }
      BiffRecordData::Selection(value) => {
        if let Some(group) = result.groups.last_mut() {
          group.selections.push(value);
        } else {
          result.orphan_view_records = true;
        }
      }
      _ => {}
    }
  }
  result
}

fn convert_sheet_properties(
  source: XlsSheetRef<'_>,
  windows: &XlsSheetWindows<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Box<SheetProperties>>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut worksheet_options = Vec::new();
  let mut code_names = Vec::new();
  let mut extensions = Vec::new();
  let mut synchronization = Vec::new();
  let mut filter_mode_count = 0usize;
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::WsBool(value) => worksheet_options.push(*value),
      BiffRecordData::CodeName(value) => code_names.push(value),
      BiffRecordData::SheetExt(value) => extensions.push(value),
      BiffRecordData::Sync(value) => synchronization.push(value),
      BiffRecordData::Empty {
        kind: EmptyRecordKind::FilterMode,
        ..
      } => filter_mode_count += 1,
      _ => {}
    }
  }
  if worksheet_options.len() != 1
    || code_names.len() > 1
    || extensions.len() > 1
    || synchronization.len() > 1
    || filter_mode_count > 1
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPropertiesNotMapped,
      location,
    )?;
  }

  let mut target = SheetProperties::default();
  let mut mapped = false;
  if let Some(source) = worksheet_options.first().copied() {
    if source.dialog {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPropertiesNotMapped,
        location,
      )?;
    }
    let right_to_left = windows
      .groups
      .first()
      .is_some_and(|group| group.window.flags.contains(Window2Flags::RIGHT_TO_LEFT));
    if windows
      .groups
      .iter()
      .any(|group| group.window.flags.contains(Window2Flags::RIGHT_TO_LEFT) != right_to_left)
    {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPropertiesNotMapped,
        location,
      )?;
    }
    target.sync_horizontal = Some(BooleanValue::from_bool(source.synchronize_horizontal));
    target.sync_vertical = Some(BooleanValue::from_bool(source.synchronize_vertical));
    target.transition_evaluation = Some(BooleanValue::from_bool(
      source.transition_formula_evaluation,
    ));
    target.transition_entry = Some(BooleanValue::from_bool(source.transition_formula_entry));
    target.outline_properties = Some(OutlineProperties {
      apply_styles: Some(BooleanValue::from_bool(source.apply_outline_styles)),
      summary_below: Some(BooleanValue::from_bool(source.summary_rows_below)),
      summary_right: Some(BooleanValue::from_bool(summary_columns_on_right(
        source.summary_columns_opposite_default_side,
        right_to_left,
      ))),
      ..Default::default()
    });
    target.page_setup_properties = Some(PageSetupProperties {
      auto_page_breaks: Some(BooleanValue::from_bool(source.show_automatic_page_breaks)),
      fit_to_page: Some(BooleanValue::from_bool(source.fit_to_page)),
    });
    if (source.synchronize_horizontal || source.synchronize_vertical) && synchronization.is_empty()
    {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPropertiesNotMapped,
        location,
      )?;
    }
    mapped = true;
  }

  if let Some(source) = synchronization.first() {
    target.sync_reference = Some(cell_reference(source.row, source.column));
    mapped = true;
  }
  if let Some(source) = code_names.first() {
    target.code_name = biff_string_text(&source.name.text);
    if target.code_name.is_none() {
      unsupported(
        report,
        options,
        ConversionCode::CompatibilityUtf16,
        location,
      )?;
    } else {
      mapped = true;
    }
  }
  if filter_mode_count == 1 {
    target.filter_mode = Some(BooleanValue::True);
    mapped = true;
  }
  if let Some(source) = extensions.first() {
    if let Some(optional) = source.optional {
      target.published = Some(BooleanValue::from_bool(!optional.flags.not_published));
      target.enable_format_conditions_calculation = Some(BooleanValue::from_bool(
        optional.flags.calculate_conditional_formats,
      ));
    }
    target.tab_color = convert_sheet_tab_color(source);
    if source.tab_color.color_index != 0x7f && target.tab_color.is_none() {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPropertiesNotMapped,
        location,
      )?;
    }
    mapped = true;
  }

  if mapped {
    report.record(Disposition::Mapped);
    Ok(Some(Box::new(target)))
  } else {
    Ok(None)
  }
}

/// MS-XLS fColSumsRight is relative to display direction rather than an
/// absolute "on the right" value like SpreadsheetML summaryRight.
const fn summary_columns_on_right(opposite_default_side: bool, right_to_left: bool) -> bool {
  opposite_default_side == right_to_left
}

fn convert_sheet_tab_color(source: &SheetExtRecord) -> Option<TabColor> {
  if source.tab_color.color_index == 0x7f {
    return None;
  }
  if let Some(optional) = source.optional
    && optional.flags.color_index == source.tab_color.color_index
  {
    return convert_cf_tab_color(optional.color);
  }
  Some(TabColor {
    indexed: Some(u32::from(source.tab_color.color_index)),
    ..Default::default()
  })
}

fn convert_cf_tab_color(source: CfColor) -> Option<TabColor> {
  let tint = f64::from_bits(source.tint_bits);
  let mut target = TabColor {
    tint: (tint != 0.0).then_some(tint),
    ..Default::default()
  };
  match source.color_type {
    0 => target.auto = Some(BooleanValue::True),
    1 => target.indexed = Some(source.color_value),
    2 => {
      let [red, green, blue, alpha] = source.color_value.to_le_bytes();
      target.rgb = Some(format!("{alpha:02X}{red:02X}{green:02X}{blue:02X}"));
    }
    3 => target.theme = Some(source.color_value),
    _ => return None,
  }
  Some(target)
}

fn convert_sheet_dimension(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SheetDimension>> {
  let dimensions = source
    .direct_records()
    .filter_map(|record| match &record.data {
      BiffRecordData::Dimensions(value) => Some(value),
      _ => None,
    })
    .collect::<Vec<_>>();
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let Some(source) = dimensions
    .first()
    .copied()
    .filter(|_| dimensions.len() == 1)
  else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetDimensionNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let reference = if source.last_row_exclusive == 0 || source.last_column_exclusive == 0 {
    "A1".to_owned()
  } else if source.first_row >= source.last_row_exclusive
    || source.first_column >= source.last_column_exclusive
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetDimensionNotMapped,
      location,
    )?;
    return Ok(None);
  } else {
    let first = cell_reference_u32(source.first_row, u32::from(source.first_column));
    let last = cell_reference_u32(
      source.last_row_exclusive - 1,
      u32::from(source.last_column_exclusive - 1),
    );
    match (first, last) {
      (Some(first), Some(last)) if first == last => first,
      (Some(first), Some(last)) => format!("{first}:{last}"),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetDimensionNotMapped,
          location,
        )?;
        return Ok(None);
      }
    }
  };
  report.record(Disposition::Mapped);
  Ok(Some(SheetDimension { reference }))
}

#[derive(Default)]
struct ConvertedPhoneticInformation {
  properties: Option<PhoneticProperties>,
  visible_ranges: Vec<CellRange>,
}

fn convert_phonetic_information(
  view: &XlsWorkbookView<'_>,
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<ConvertedPhoneticInformation> {
  let values = source
    .direct_records()
    .filter_map(|record| match &record.data {
      BiffRecordData::PhoneticInfo(value) => Some(value),
      _ => None,
    })
    .collect::<Vec<_>>();
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let Some(source) = values.first().copied().filter(|_| values.len() == 1) else {
    if values.len() > 1 {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPhoneticInformationNotMapped,
        location,
      )?;
    }
    return Ok(ConvertedPhoneticInformation::default());
  };
  let font_id = font_position(source.font_index)
    .filter(|_| view.font(source.font_index).is_some())
    .and_then(|value| u32::try_from(value).ok());
  let Some(font_id) = font_id else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPhoneticInformationNotMapped,
      location,
    )?;
    return Ok(ConvertedPhoneticInformation {
      properties: None,
      visible_ranges: source.ranges.clone(),
    });
  };
  let r#type = match source.flags.phonetic_type() {
    PhoneticType::NarrowKatakana => PhoneticValues::HalfWidthKatakana,
    PhoneticType::WideKatakana => PhoneticValues::FullWidthKatakana,
    PhoneticType::Hiragana => PhoneticValues::Hiragana,
    PhoneticType::Any => PhoneticValues::NoConversion,
  };
  let alignment = match source.flags.alignment() {
    PhoneticAlignment::General => PhoneticAlignmentValues::NoControl,
    PhoneticAlignment::Left => PhoneticAlignmentValues::Left,
    PhoneticAlignment::Center => PhoneticAlignmentValues::Center,
    PhoneticAlignment::Distributed => PhoneticAlignmentValues::Distributed,
  };
  report.record(Disposition::Mapped);
  Ok(ConvertedPhoneticInformation {
    properties: Some(PhoneticProperties {
      font_id,
      r#type: Some(r#type),
      alignment: Some(alignment),
    }),
    visible_ranges: source.ranges.clone(),
  })
}

fn phonetic_is_visible(ranges: &[CellRange], row: u16, column: u16) -> bool {
  ranges.iter().any(|range| {
    (range.first_row..=range.last_row).contains(&row)
      && (range.first_column..=range.last_column).contains(&column)
  })
}

fn convert_workbook_views(
  view: &XlsWorkbookView<'_>,
  target_sheet_positions: &[Option<u32>],
  sheet_windows: &[XlsSheetWindows<'_>],
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Vec<WorkbookView>> {
  let source = view
    .globals_records()
    .iter()
    .filter_map(|record| match &record.data {
      BiffRecordData::Window1(value) => Some(value),
      _ => None,
    })
    .collect::<Vec<_>>();
  if source.is_empty() {
    unsupported(
      report,
      options,
      ConversionCode::WorkbookViewNotMapped,
      SourceLocation::XlsWorkbook { workbook_index: 0 },
    )?;
    return Ok(Vec::new());
  }

  source
    .into_iter()
    .enumerate()
    .map(|(view_index, source)| {
      let active_source = usize::from(source.active_sheet);
      let first_source = usize::from(source.first_visible_tab);
      let active_tab = target_sheet_positions.get(active_source).copied().flatten();
      let first_sheet = target_sheet_positions.get(first_source).copied().flatten();
      let active_window_sheets = sheet_windows
        .iter()
        .enumerate()
        .filter_map(|(sheet_index, windows)| {
          windows
            .groups
            .get(view_index)
            .filter(|group| group.window.flags.contains(Window2Flags::ACTIVE))
            .map(|_| sheet_index)
        })
        .collect::<Vec<_>>();
      let selected_window_count = sheet_windows
        .iter()
        .filter(|windows| {
          windows
            .groups
            .get(view_index)
            .is_some_and(|group| group.window.flags.contains(Window2Flags::SELECTED))
        })
        .count();
      let invalid = source.width < 1
        || source.height < 1
        || source.tab_width_ratio > 1000
        || active_tab.is_none()
        || first_sheet.is_none()
        || active_window_sheets.as_slice() != [active_source]
        || selected_window_count != usize::from(source.selected_tab_count);
      if invalid {
        unsupported(
          report,
          options,
          ConversionCode::WorkbookViewNotMapped,
          SourceLocation::XlsWorkbook { workbook_index: 0 },
        )?;
      }
      let visibility = if source.flags & 0x0004 != 0 {
        VisibilityValues::VeryHidden
      } else if source.flags & 0x0001 != 0 {
        VisibilityValues::Hidden
      } else {
        VisibilityValues::Visible
      };
      report.record(Disposition::Mapped);
      Ok(WorkbookView {
        visibility: Some(visibility),
        minimized: bool_value(source.flags & 0x0002 != 0),
        show_horizontal_scroll: bool_value(source.flags & 0x0008 != 0),
        show_vertical_scroll: bool_value(source.flags & 0x0010 != 0),
        show_sheet_tabs: bool_value(source.flags & 0x0020 != 0),
        x_window: Some(i32::from(source.horizontal_position)),
        y_window: Some(i32::from(source.vertical_position)),
        window_width: u32::try_from(source.width).ok(),
        window_height: u32::try_from(source.height).ok(),
        tab_ratio: (source.tab_width_ratio <= 1000).then_some(u32::from(source.tab_width_ratio)),
        first_sheet,
        active_tab,
        auto_filter_date_grouping: bool_value(source.flags & 0x0040 == 0),
        ..Default::default()
      })
    })
    .collect()
}

fn convert_sheet_format_properties(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SheetFormatProperties>> {
  let mut default_rows = Vec::new();
  let mut base_widths = Vec::new();
  let mut default_widths = Vec::new();
  let mut guts = Vec::new();
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::DefaultRowHeight(value) => default_rows.push(value),
      BiffRecordData::FixedU16 {
        kind: FixedU16RecordKind::DefaultColWidth,
        value,
      } => base_widths.push(*value),
      BiffRecordData::FixedU16 {
        kind: FixedU16RecordKind::StandardWidth,
        value,
      } => default_widths.push(*value),
      BiffRecordData::Guts(value) => guts.push(value),
      _ => {}
    }
  }
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let Some(default_row) = default_rows
    .first()
    .copied()
    .filter(|_| default_rows.len() == 1)
  else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetDefaultFormattingNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let hidden = default_row.flags & 0x0002 != 0;
  let valid_height = if hidden {
    (0..=8179).contains(&default_row.height)
  } else {
    (1..=8179).contains(&default_row.height)
  };
  let base_column_width = match base_widths.as_slice() {
    [value] if *value <= 255 => Some(u32::from(*value)),
    [value] => {
      let _ = value;
      unsupported(
        report,
        options,
        ConversionCode::WorksheetDefaultFormattingNotMapped,
        location,
      )?;
      None
    }
    [] => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetDefaultFormattingNotMapped,
        location,
      )?;
      None
    }
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetDefaultFormattingNotMapped,
        location,
      )?;
      None
    }
  };
  let default_column_width = match default_widths.as_slice() {
    [value] => Some(f64::from(*value) / 256.0),
    [] => None,
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetDefaultFormattingNotMapped,
        location,
      )?;
      None
    }
  };
  if !valid_height {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetDefaultFormattingNotMapped,
      location,
    )?;
    return Ok(None);
  }
  let actual_outline_level_row = source
    .row_records()
    .map(|row| u8::try_from(row.flags & 0x0000_0007).expect("three bits fit u8"))
    .max()
    .filter(|value| *value != 0);
  let actual_outline_level_column = source
    .column_infos()
    .map(|column| u8::try_from((column.flags >> 8) & 0x0007).expect("three bits fit u8"))
    .max()
    .filter(|value| *value != 0);
  let (outline_level_row, outline_level_column) = match guts.as_slice() {
    [] => (actual_outline_level_row, actual_outline_level_column),
    [guts] => {
      if !guts_outline_level_is_valid(guts.maximum_row_outline_level)
        || !guts_outline_level_is_valid(guts.maximum_column_outline_level)
      {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetDefaultFormattingNotMapped,
          location,
        )?;
        (actual_outline_level_row, actual_outline_level_column)
      } else {
        let declared_row = guts_outline_level(guts.maximum_row_outline_level);
        let declared_column = guts_outline_level(guts.maximum_column_outline_level);
        if declared_row != actual_outline_level_row
          || declared_column != actual_outline_level_column
        {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetDefaultFormattingNotMapped,
            location,
          )?;
        }
        (declared_row, declared_column)
      }
    }
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetDefaultFormattingNotMapped,
        location,
      )?;
      (actual_outline_level_row, actual_outline_level_column)
    }
  };
  report.record(Disposition::Mapped);
  Ok(Some(SheetFormatProperties {
    base_column_width,
    default_column_width,
    default_row_height: f64::from(default_row.height) / 20.0,
    custom_height: bool_attribute(default_row.flags & 0x0001 != 0),
    zero_height: bool_attribute(hidden),
    thick_top: bool_attribute(default_row.flags & 0x0004 != 0),
    thick_bottom: bool_attribute(default_row.flags & 0x0008 != 0),
    outline_level_row,
    outline_level_column,
    ..Default::default()
  }))
}

const fn guts_outline_level(value: u16) -> Option<u8> {
  match value {
    2..=8 => Some((value - 1) as u8),
    _ => None,
  }
}

const fn guts_outline_level_is_valid(value: u16) -> bool {
  matches!(value, 0 | 2..=8)
}

fn convert_sheet_views(
  source: &XlsSheetWindows<'_>,
  workbook_view_count: usize,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Box<SheetViews>>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  if source.orphan_view_records || source.groups.len() != workbook_view_count {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetViewNotMapped,
      location,
    )?;
  }
  let mut sheet_view = Vec::with_capacity(source.groups.len().min(workbook_view_count));
  for (view_index, source) in source.groups.iter().take(workbook_view_count).enumerate() {
    let page_layout = match source.page_layout.as_slice() {
      [] => None,
      [value] => Some(*value),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetViewNotMapped,
          location,
        )?;
        None
      }
    };
    let scale = match source.scale.as_slice() {
      [] => None,
      [value] => Some(*value),
      _ => {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetViewNotMapped,
          location,
        )?;
        None
      }
    };
    let flags = source.window.flags;
    let page_break_preview = flags.contains(Window2Flags::PAGE_BREAK_PREVIEW);
    let page_layout_view =
      page_layout.is_some_and(|value| value.flags.contains(PlvFlags::PAGE_LAYOUT_VIEW));
    let view = if page_break_preview && page_layout_view {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetViewNotMapped,
        location,
      )?;
      None
    } else if page_break_preview {
      Some(SheetViewValues::PageBreakPreview)
    } else if page_layout_view {
      Some(SheetViewValues::PageLayout)
    } else {
      Some(SheetViewValues::Normal)
    };
    let valid_top_left = source.window.left_column <= 255
      && (!flags.contains(Window2Flags::FREEZE_PANES)
        || (source.window.left_column != 255 && source.window.top_row != u16::MAX));
    let valid_grid_color = source.window.header_color <= 64
      && (flags.contains(Window2Flags::DEFAULT_HEADER) == (source.window.header_color == 64));
    if !valid_top_left
      || !valid_grid_color
      || (flags.contains(Window2Flags::FREEZE_NO_SPLIT)
        && !flags.contains(Window2Flags::FREEZE_PANES))
    {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetViewNotMapped,
        location,
      )?;
    }
    let (page_break_zoom, normal_zoom) = match source.window.extension {
      Window2Extension::None => (None, None),
      Window2Extension::Zoom {
        page_break_zoom,
        normal_zoom,
        ..
      } => {
        if !valid_saved_zoom(page_break_zoom) || !valid_saved_zoom(normal_zoom) {
          unsupported(
            report,
            options,
            ConversionCode::WorksheetViewNotMapped,
            location,
          )?;
        }
        (
          nonzero_valid_zoom(page_break_zoom),
          nonzero_valid_zoom(normal_zoom),
        )
      }
    };
    let zoom_scale = scale
      .map(|scale| convert_current_zoom(scale, location, options, report))
      .transpose()?
      .flatten();
    let page_layout_zoom = if let Some(page_layout) = page_layout {
      if !valid_saved_zoom(page_layout.zoom_scale) {
        unsupported(
          report,
          options,
          ConversionCode::WorksheetViewNotMapped,
          location,
        )?;
        None
      } else {
        nonzero_valid_zoom(page_layout.zoom_scale)
      }
    } else {
      None
    };
    let pane = convert_pane(source, sheet_index, options, report)?;
    let selection = convert_selections(&source.selections, sheet_index, options, report)?;
    sheet_view.push(SheetView {
      show_formulas: bool_value(flags.contains(Window2Flags::DISPLAY_FORMULAS)),
      show_grid_lines: bool_value(flags.contains(Window2Flags::DISPLAY_GRIDLINES)),
      show_row_col_headers: bool_value(flags.contains(Window2Flags::DISPLAY_ROW_COLUMN_HEADINGS)),
      show_zeros: bool_value(flags.contains(Window2Flags::DISPLAY_ZEROS)),
      right_to_left: bool_value(flags.contains(Window2Flags::RIGHT_TO_LEFT)),
      tab_selected: bool_value(flags.contains(Window2Flags::SELECTED)),
      show_ruler: page_layout
        .map(|value| BooleanValue::from_bool(value.flags.contains(PlvFlags::RULER_VISIBLE))),
      show_outline_symbols: bool_value(flags.contains(Window2Flags::DISPLAY_OUTLINE)),
      default_grid_color: bool_value(flags.contains(Window2Flags::DEFAULT_HEADER)),
      show_white_space: page_layout
        .map(|value| BooleanValue::from_bool(!value.flags.contains(PlvFlags::WHITESPACE_HIDDEN))),
      view,
      top_left_cell: valid_top_left
        .then(|| cell_reference(source.window.top_row, source.window.left_column)),
      color_id: valid_grid_color.then_some(u32::from(source.window.header_color)),
      zoom_scale,
      zoom_scale_normal: normal_zoom,
      zoom_scale_sheet_layout_view: page_break_zoom,
      zoom_scale_page_layout_view: page_layout_zoom,
      workbook_view_id: u32::try_from(view_index)
        .map_err(|_| olecfsdk::Error::Limit("XLS workbook view index exceeds u32".into()))?,
      pane,
      selection,
      ..Default::default()
    });
    report.record(Disposition::Mapped);
  }
  Ok((!sheet_view.is_empty()).then(|| {
    Box::new(SheetViews {
      sheet_view,
      ..Default::default()
    })
  }))
}

#[derive(Debug, Default)]
struct XlsPageSettings {
  print_options: Option<PrintOptions>,
  page_margins: Option<PageMargins>,
  page_setup: Option<PageSetup>,
  header_footer: Option<Box<HeaderFooter>>,
  row_breaks: Option<RowBreaks>,
  column_breaks: Option<ColumnBreaks>,
}

fn convert_page_settings(
  source: XlsSheetRef<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<XlsPageSettings> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut print_headers = Vec::new();
  let mut print_gridlines = Vec::new();
  let mut gridsets = Vec::new();
  let mut horizontal_centers = Vec::new();
  let mut vertical_centers = Vec::new();
  let mut left_margins = Vec::new();
  let mut right_margins = Vec::new();
  let mut top_margins = Vec::new();
  let mut bottom_margins = Vec::new();
  let mut setups = Vec::new();
  let mut headers = Vec::new();
  let mut footers = Vec::new();
  let mut extended = Vec::new();
  let mut row_break_records = Vec::new();
  let mut column_break_records = Vec::new();
  for record in source.direct_records() {
    match &record.data {
      BiffRecordData::FixedU16 { kind, value } => match kind {
        FixedU16RecordKind::PrintHeaders => print_headers.push(*value),
        FixedU16RecordKind::PrintGridlines => print_gridlines.push(*value),
        FixedU16RecordKind::Gridset => gridsets.push(*value),
        FixedU16RecordKind::HCenter => horizontal_centers.push(*value),
        FixedU16RecordKind::VCenter => vertical_centers.push(*value),
        _ => {}
      },
      BiffRecordData::FixedF64Bits { kind, bits } => match kind {
        FixedF64RecordKind::LeftMargin => left_margins.push(*bits),
        FixedF64RecordKind::RightMargin => right_margins.push(*bits),
        FixedF64RecordKind::TopMargin => top_margins.push(*bits),
        FixedF64RecordKind::BottomMargin => bottom_margins.push(*bits),
        _ => {}
      },
      BiffRecordData::PrintSetup(value) => setups.push(value),
      BiffRecordData::Header(value) => headers.push(value),
      BiffRecordData::Footer(value) => footers.push(value),
      BiffRecordData::ExtendedHeaderFooter(value) if value.sheet_view_guid == [0; 16] => {
        extended.push(value)
      }
      BiffRecordData::HorizontalPageBreaks(value) => row_break_records.push(value),
      BiffRecordData::VerticalPageBreaks(value) => column_break_records.push(value),
      _ => {}
    }
  }

  let setup = match setups.as_slice() {
    [value] => Some(*value),
    _ => {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPageSetupNotMapped,
        location,
      )?;
      None
    }
  };
  Ok(XlsPageSettings {
    print_options: convert_print_options(
      &print_headers,
      &print_gridlines,
      &gridsets,
      &horizontal_centers,
      &vertical_centers,
      location,
      options,
      report,
    )?,
    page_margins: convert_page_margins(
      &left_margins,
      &right_margins,
      &top_margins,
      &bottom_margins,
      setup,
      location,
      options,
      report,
    )?,
    page_setup: setup
      .map(|value| convert_page_setup(value, location, options, report))
      .transpose()?,
    header_footer: convert_header_footer(&headers, &footers, &extended, location, options, report)?,
    row_breaks: convert_row_breaks(&row_break_records, location, options, report)?,
    column_breaks: convert_column_breaks(&column_break_records, location, options, report)?,
  })
}

#[allow(clippy::too_many_arguments)]
fn convert_print_options(
  print_headers: &[u16],
  print_gridlines: &[u16],
  gridsets: &[u16],
  horizontal_centers: &[u16],
  vertical_centers: &[u16],
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<PrintOptions>> {
  let cardinality_valid = [
    print_headers.len(),
    print_gridlines.len(),
    gridsets.len(),
    horizontal_centers.len(),
    vertical_centers.len(),
  ]
  .into_iter()
  .all(|count| count == 1);
  if !cardinality_valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPrintOptionsNotMapped,
      location,
    )?;
    return Ok(None);
  }
  let headings = print_headers[0];
  let grid_lines = print_gridlines[0];
  let grid_lines_set = gridsets[0];
  let horizontal_centered = horizontal_centers[0];
  let vertical_centered = vertical_centers[0];
  if headings > 1 || grid_lines_set > 1 || horizontal_centered > 1 || vertical_centered > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPrintOptionsNotMapped,
      location,
    )?;
    return Ok(None);
  }
  report.record(Disposition::Mapped);
  Ok(Some(PrintOptions {
    horizontal_centered: bool_value(horizontal_centered == 1),
    vertical_centered: bool_value(vertical_centered == 1),
    headings: bool_value(headings == 1),
    // PrintGrid's upper fifteen bits are explicitly unused by MS-XLS.
    grid_lines: bool_value(grid_lines & 1 != 0),
    grid_lines_set: bool_value(grid_lines_set == 1),
  }))
}

#[allow(clippy::too_many_arguments)]
fn convert_page_margins(
  left: &[u64],
  right: &[u64],
  top: &[u64],
  bottom: &[u64],
  setup: Option<&PrintSetupRecord>,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<PageMargins>> {
  let Some(setup) = setup else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageMarginsNotMapped,
      location,
    )?;
    return Ok(None);
  };
  // MS-XLS makes the four side-margin records optional. Both LibreOffice and
  // POI apply these BIFF defaults when a record is absent.
  let margin = |values: &[u64], default: f64| match values {
    [] => Some(default),
    [bits] => Some(f64::from_bits(*bits)),
    _ => None,
  };
  let values = (
    margin(left, 0.75),
    margin(right, 0.75),
    margin(top, 1.0),
    margin(bottom, 1.0),
    Some(f64::from_bits(setup.header_margin_bits)),
    Some(f64::from_bits(setup.footer_margin_bits)),
  );
  let (Some(left), Some(right), Some(top), Some(bottom), Some(header), Some(footer)) = values
  else {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageMarginsNotMapped,
      location,
    )?;
    return Ok(None);
  };
  let valid = [left, right, top, bottom]
    .into_iter()
    .all(|value| value.is_finite() && (0.0..=49.0).contains(&value))
    && [header, footer]
      .into_iter()
      .all(|value| value.is_finite() && (0.0..49.0).contains(&value));
  if !valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageMarginsNotMapped,
      location,
    )?;
    return Ok(None);
  }
  report.record(Disposition::Mapped);
  Ok(Some(PageMargins {
    left,
    right,
    top,
    bottom,
    header,
    footer,
  }))
}

fn convert_page_setup(
  source: &PrintSetupRecord,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<PageSetup> {
  let source_options = source.options;
  let printer_values_defined = !source_options.no_printer_settings;
  let paper_size_valid = !printer_values_defined || !(118..=255).contains(&source.paper_size);
  let scale_valid = !printer_values_defined || (10..=400).contains(&source.scale);
  let fit_width_valid = source.fit_width <= 32767;
  let fit_height_valid = source.fit_height <= 32767;
  if source_options.reserved != 0
    || !paper_size_valid
    || !scale_valid
    || !fit_width_valid
    || !fit_height_valid
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageSetupNotMapped,
      location,
    )?;
  }
  let cell_comments = if !source_options.print_comments {
    CellCommentsValues::None
  } else if source_options.comments_at_end {
    CellCommentsValues::AtEnd
  } else {
    CellCommentsValues::AsDisplayed
  };
  let errors = match source_options.print_errors {
    0 => PrintErrorValues::Displayed,
    1 => PrintErrorValues::Blank,
    2 => PrintErrorValues::Dash,
    3 => PrintErrorValues::Na,
    _ => unreachable!("two-bit print error value"),
  };
  let orientation = printer_values_defined.then_some(if source_options.no_orientation {
    // MS-XLS explicitly requires portrait output when fNoOrient is set.
    OrientationValues::Portrait
  } else if source_options.portrait {
    OrientationValues::Portrait
  } else {
    OrientationValues::Landscape
  });
  report.record(Disposition::Mapped);
  Ok(PageSetup {
    paper_size: (printer_values_defined && paper_size_valid)
      .then_some(u32::from(source.paper_size)),
    scale: (printer_values_defined && scale_valid).then_some(u32::from(source.scale)),
    first_page_number: source_options
      .use_first_page_number
      .then_some(i64::from(source.page_start)),
    fit_to_width: fit_width_valid.then_some(u32::from(source.fit_width)),
    fit_to_height: fit_height_valid.then_some(u32::from(source.fit_height)),
    page_order: Some(if source_options.left_to_right {
      PageOrderValues::OverThenDown
    } else {
      PageOrderValues::DownThenOver
    }),
    orientation,
    use_printer_defaults: bool_value(source_options.no_printer_settings),
    black_and_white: bool_value(source_options.black_and_white),
    draft: bool_value(source_options.draft),
    cell_comments: Some(cell_comments),
    use_first_page_number: bool_value(source_options.use_first_page_number),
    errors: Some(errors),
    horizontal_dpi: printer_values_defined.then_some(u32::from(source.horizontal_resolution)),
    vertical_dpi: printer_values_defined.then_some(u32::from(source.vertical_resolution)),
    copies: printer_values_defined.then_some(u32::from(source.copies)),
    ..Default::default()
  })
}

fn convert_header_footer(
  headers: &[&HeaderFooterRecord],
  footers: &[&HeaderFooterRecord],
  extended: &[&olecfsdk::xls::ExtendedHeaderFooterRecord],
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Box<HeaderFooter>>> {
  if headers.len() != 1 || footers.len() != 1 || extended.len() > 1 {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetHeaderFooterNotMapped,
      location,
    )?;
  }
  let odd_header = headers.first().and_then(|value| header_footer_text(value));
  let odd_footer = footers.first().and_then(|value| header_footer_text(value));
  if headers
    .first()
    .is_some_and(|value| matches!(value, HeaderFooterRecord::Text { .. }) && odd_header.is_none())
    || footers
      .first()
      .is_some_and(|value| matches!(value, HeaderFooterRecord::Text { .. }) && odd_footer.is_none())
  {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetHeaderFooterNotMapped,
      location,
    )?;
  }

  let mut target = HeaderFooter {
    odd_header: odd_header.map(|value| OddHeader(xstring(value))),
    odd_footer: odd_footer.map(|value| OddFooter(xstring(value))),
    ..Default::default()
  };
  if let Some(source) = extended.first() {
    let different_odd_even = source
      .flags
      .contains(ExtendedHeaderFooterFlags::DIFFERENT_ODD_EVEN);
    let different_first = source
      .flags
      .contains(ExtendedHeaderFooterFlags::DIFFERENT_FIRST);
    let even_header = source.even_header.as_ref().and_then(biff_string_text);
    let even_footer = source.even_footer.as_ref().and_then(biff_string_text);
    let first_header = source.first_header.as_ref().and_then(biff_string_text);
    let first_footer = source.first_footer.as_ref().and_then(biff_string_text);
    let invalid_text = [
      (source.even_header.as_ref(), even_header.as_ref()),
      (source.even_footer.as_ref(), even_footer.as_ref()),
      (source.first_header.as_ref(), first_header.as_ref()),
      (source.first_footer.as_ref(), first_footer.as_ref()),
    ]
    .into_iter()
    .any(|(source, target)| source.is_some() && target.is_none());
    let invalid_flags = (!different_odd_even
      && (source.even_header.is_some() || source.even_footer.is_some()))
      || (!different_first && (source.first_header.is_some() || source.first_footer.is_some()));
    let invalid_counts = [
      source.even_header_character_count,
      source.even_footer_character_count,
      source.first_header_character_count,
      source.first_footer_character_count,
    ]
    .into_iter()
    .any(|count| count > 255);
    if invalid_text || invalid_flags || invalid_counts {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetHeaderFooterNotMapped,
        location,
      )?;
    }
    target.different_odd_even = bool_value(different_odd_even);
    target.different_first = bool_value(different_first);
    target.scale_with_doc = bool_value(
      source
        .flags
        .contains(ExtendedHeaderFooterFlags::SCALE_WITH_DOCUMENT),
    );
    target.align_with_margins = bool_value(
      source
        .flags
        .contains(ExtendedHeaderFooterFlags::ALIGN_MARGINS),
    );
    if different_odd_even {
      target.even_header = even_header.map(|value| EvenHeader(xstring(value)));
      target.even_footer = even_footer.map(|value| EvenFooter(xstring(value)));
    }
    if different_first {
      target.first_header = first_header.map(|value| FirstHeader(xstring(value)));
      target.first_footer = first_footer.map(|value| FirstFooter(xstring(value)));
    }
  }
  let present = target.odd_header.is_some()
    || target.odd_footer.is_some()
    || target.even_header.is_some()
    || target.even_footer.is_some()
    || target.first_header.is_some()
    || target.first_footer.is_some()
    || !extended.is_empty();
  report.record(Disposition::Mapped);
  Ok(present.then(|| Box::new(target)))
}

fn header_footer_text(value: &HeaderFooterRecord) -> Option<String> {
  match value {
    HeaderFooterRecord::EmptyPayload | HeaderFooterRecord::EmptyCountOnly => None,
    HeaderFooterRecord::Text { characters, .. } => formula_string(characters),
  }
}

fn biff_string_text(value: &BiffUnicodeString) -> Option<String> {
  formula_string(&value.characters)
}

fn convert_row_breaks(
  source: &[&olecfsdk::xls::HorizontalPageBreaksRecord],
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<RowBreaks>> {
  let Some(source) = source.first().copied().filter(|_| source.len() == 1) else {
    if source.len() > 1 {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPageBreaksNotMapped,
        location,
      )?;
    }
    return Ok(None);
  };
  let valid = source.breaks.len() <= 1023
    && source
      .breaks
      .iter()
      .all(|value| value.first_column < value.last_column && value.last_column <= 16383)
    && source.breaks.windows(2).all(|values| {
      values[0].row < values[1].row
        || (values[0].row == values[1].row && values[0].last_column < values[1].first_column)
    });
  if !valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageBreaksNotMapped,
      location,
    )?;
    return Ok(None);
  }
  if source.breaks.is_empty() {
    return Ok(None);
  }
  let count = u32::try_from(source.breaks.len())
    .map_err(|_| olecfsdk::Error::Limit("XLS row page-break count exceeds u32".into()))?;
  report.record(Disposition::Mapped);
  Ok(Some(RowBreaks {
    count: Some(count),
    manual_break_count: Some(count),
    r#break: source
      .breaks
      .iter()
      .map(|value| Break {
        id: Some(u32::from(value.row)),
        min: Some(u32::from(value.first_column)),
        max: Some(u32::from(value.last_column)),
        manual_page_break: bool_value(true),
        ..Default::default()
      })
      .collect(),
  }))
}

fn convert_column_breaks(
  source: &[&olecfsdk::xls::VerticalPageBreaksRecord],
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<ColumnBreaks>> {
  let Some(source) = source.first().copied().filter(|_| source.len() == 1) else {
    if source.len() > 1 {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPageBreaksNotMapped,
        location,
      )?;
    }
    return Ok(None);
  };
  let valid = source.breaks.len() <= 255
    && source
      .breaks
      .iter()
      .all(|value| value.column <= 255 && value.first_row < value.last_row)
    && source.breaks.windows(2).all(|values| {
      values[0].column < values[1].column
        || (values[0].column == values[1].column && values[0].last_row < values[1].first_row)
    });
  if !valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPageBreaksNotMapped,
      location,
    )?;
    return Ok(None);
  }
  if source.breaks.is_empty() {
    return Ok(None);
  }
  let count = u32::try_from(source.breaks.len())
    .map_err(|_| olecfsdk::Error::Limit("XLS column page-break count exceeds u32".into()))?;
  report.record(Disposition::Mapped);
  Ok(Some(ColumnBreaks {
    count: Some(count),
    manual_break_count: Some(count),
    r#break: source
      .breaks
      .iter()
      .map(|value| Break {
        id: Some(u32::from(value.column)),
        min: Some(u32::from(value.first_row)),
        max: Some(u32::from(value.last_row)),
        manual_page_break: bool_value(true),
        ..Default::default()
      })
      .collect(),
  }))
}

fn valid_saved_zoom(value: u16) -> bool {
  value == 0 || (10..=400).contains(&value)
}

fn nonzero_valid_zoom(value: u16) -> Option<u32> {
  (value != 0 && valid_saved_zoom(value)).then_some(u32::from(value))
}

fn convert_current_zoom(
  source: &SclRecord,
  location: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<u32>> {
  let numerator = u32::from(source.numerator);
  let denominator = u32::from(source.denominator);
  let scaled = numerator.saturating_mul(100);
  let valid = denominator != 0
    && numerator.saturating_mul(10) >= denominator
    && numerator <= denominator.saturating_mul(4)
    && scaled % denominator == 0;
  if !valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetViewNotMapped,
      location,
    )?;
    return Ok(None);
  }
  Ok(Some(scaled / denominator))
}

fn convert_pane(
  group: &XlsWindowGroup<'_>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<Pane>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let Some(source) = group
    .pane
    .first()
    .copied()
    .filter(|_| group.pane.len() == 1)
  else {
    if !group.pane.is_empty()
      || group.window.flags.contains(Window2Flags::FREEZE_PANES)
      || group.window.flags.contains(Window2Flags::FREEZE_NO_SPLIT)
    {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetPaneNotMapped,
        location,
      )?;
    }
    return Ok(None);
  };
  let frozen = group.window.flags.contains(Window2Flags::FREEZE_PANES);
  let active_pane = pane_value(source.active_pane);
  let valid = active_pane.is_some()
    && source.left_column <= 255
    && if frozen {
      source.horizontal_split <= 255
    } else {
      source.horizontal_split <= 32767 && source.vertical_split <= 32767
    };
  if !valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetPaneNotMapped,
      location,
    )?;
    return Ok(None);
  }
  let state = if frozen {
    if group.window.flags.contains(Window2Flags::FREEZE_NO_SPLIT) {
      PaneStateValues::Frozen
    } else {
      PaneStateValues::FrozenSplit
    }
  } else {
    PaneStateValues::Split
  };
  report.record(Disposition::Mapped);
  Ok(Some(Pane {
    horizontal_split: Some(f64::from(source.horizontal_split)),
    vertical_split: Some(f64::from(source.vertical_split)),
    top_left_cell: Some(cell_reference(source.top_row, source.left_column)),
    active_pane,
    state: Some(state),
  }))
}

fn pane_value(value: u8) -> Option<PaneValues> {
  match value {
    0 => Some(PaneValues::BottomRight),
    1 => Some(PaneValues::TopRight),
    2 => Some(PaneValues::BottomLeft),
    3 => Some(PaneValues::TopLeft),
    _ => None,
  }
}

fn convert_selections(
  source: &[&SelectionRecord],
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Vec<Selection>> {
  let location = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index,
  };
  let mut groups: Vec<Vec<&SelectionRecord>> = Vec::new();
  for source in source {
    if groups
      .last()
      .and_then(|group| group.first())
      .is_some_and(|previous| previous.pane == source.pane)
    {
      groups
        .last_mut()
        .expect("selection group exists")
        .push(source);
    } else {
      groups.push(vec![source]);
    }
  }
  let mut seen_panes = [false; 4];
  let structurally_valid = groups.len() <= 4
    && groups.iter().all(|group| {
      let pane = usize::from(group[0].pane);
      if pane >= seen_panes.len() || seen_panes[pane] {
        false
      } else {
        seen_panes[pane] = true;
        true
      }
    });
  if !structurally_valid {
    unsupported(
      report,
      options,
      ConversionCode::WorksheetSelectionNotMapped,
      location,
    )?;
    return Ok(Vec::new());
  }
  let mut result = Vec::with_capacity(groups.len());
  for group in groups {
    let first = group[0];
    let mut references = Vec::new();
    let mut valid = pane_value(first.pane).is_some()
      && first.active_column <= 255
      && first.active_reference_index >= 0
      && group.iter().all(|value| {
        value.active_row == first.active_row
          && value.active_column == first.active_column
          && value.active_reference_index == first.active_reference_index
          && usize::from(value.reference_count) == value.references.len()
      });
    for record in &group {
      for reference in &record.references {
        valid &= reference.first_row <= reference.last_row
          && reference.first_column <= reference.last_column;
        references.push(selection_reference(
          reference.first_row,
          u16::from(reference.first_column),
          reference.last_row,
          u16::from(reference.last_column),
        ));
      }
    }
    let active_reference_index = usize::try_from(first.active_reference_index).ok();
    valid &= active_reference_index.is_some_and(|index| {
      group
        .iter()
        .flat_map(|value| &value.references)
        .nth(index)
        .is_some_and(|reference| {
          (reference.first_row..=reference.last_row).contains(&first.active_row)
            && (u16::from(reference.first_column)..=u16::from(reference.last_column))
              .contains(&first.active_column)
        })
    });
    if !valid {
      unsupported(
        report,
        options,
        ConversionCode::WorksheetSelectionNotMapped,
        location,
      )?;
      continue;
    }
    result.push(Selection {
      pane: pane_value(first.pane),
      active_cell: Some(cell_reference(first.active_row, first.active_column)),
      active_cell_id: active_reference_index.and_then(|value| u32::try_from(value).ok()),
      sequence_of_references: Some(references),
    });
    report.record(Disposition::Mapped);
  }
  Ok(result)
}

fn selection_reference(
  first_row: u16,
  first_column: u16,
  last_row: u16,
  last_column: u16,
) -> String {
  if first_row == last_row && first_column == last_column {
    cell_reference(first_row, first_column)
  } else {
    cell_range_reference(first_row, first_column, last_row, last_column)
  }
}

const fn bool_value(value: bool) -> Option<BooleanValue> {
  Some(BooleanValue::from_bool(value))
}

fn convert_columns<'a>(
  source: impl Iterator<Item = &'a ColInfoRecord>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
  xf_unmapped: &[bool],
) -> Result<Vec<Column>> {
  // [MS-XLS] 2.4.53 and ECMA-376 Part 1 18.3.1.13 describe the same
  // one-based column range, 1/256-character width, XF, visibility, phonetic,
  // best-fit, and outline state represented here.
  let columns = source
    .map(|source| {
      let location = SourceLocation::XlsColumns {
        workbook_index: 0,
        sheet_index,
        first_column: source.first_column,
        last_column: source.last_column,
      };
      let valid_range = source.first_column <= source.last_column && source.last_column <= 0x00ff;
      let style = xf_unmapped.get(usize::from(source.format_index));
      if !valid_range || style.copied().unwrap_or(true) {
        unsupported(
          report,
          options,
          ConversionCode::ColumnFormattingNotMapped,
          location,
        )?;
      }
      if !valid_range {
        return Ok(None);
      }
      let flags = source.flags;
      report.record(Disposition::Mapped);
      Ok(Some(Column {
        min: u32::from(source.first_column) + 1,
        max: u32::from(source.last_column) + 1,
        width: Some(f64::from(source.width) / 256.0),
        style: style.is_some().then_some(u32::from(source.format_index)),
        hidden: bool_attribute(flags & 0x0001 != 0),
        custom_width: bool_attribute(flags & 0x0002 != 0),
        best_fit: bool_attribute(flags & 0x0004 != 0),
        phonetic: bool_attribute(flags & 0x0008 != 0),
        outline_level: nonzero_u8((flags >> 8) & 0x0007),
        collapsed: bool_attribute(flags & 0x1000 != 0),
      }))
    })
    .collect::<Result<Vec<_>>>()?;
  Ok(columns.into_iter().flatten().collect())
}

fn convert_row(
  row: u16,
  source: Option<&RowRecord>,
  cell: Vec<Cell>,
  sheet_index: usize,
  options: ConversionOptions,
  report: &mut ConversionReport,
  xf_unmapped: &[bool],
) -> Result<Row> {
  let Some(source) = source else {
    return Ok(Row {
      row_index: Some(u32::from(row) + 1),
      cell,
      ..Default::default()
    });
  };
  let flags = source.flags;
  // [MS-XLS] 2.4.221 stores height in twips and packs outline/visibility,
  // manual-height, row-XF, border and phonetic flags. ECMA-376 Part 1
  // 18.3.1.73 has direct attributes for every one of those values.
  let has_style = flags & 0x0000_0080 != 0;
  let style_index = u16::try_from((flags >> 16) & 0x0fff).expect("twelve bits fit u16");
  let valid_height = (2..=8192).contains(&source.height);
  let style = has_style.then(|| xf_unmapped.get(usize::from(style_index)));
  let style_unmapped = style.is_some_and(|style| style.copied().unwrap_or(true));
  if !valid_height || style_unmapped {
    unsupported(
      report,
      options,
      ConversionCode::RowFormattingNotMapped,
      SourceLocation::XlsRow {
        workbook_index: 0,
        sheet_index,
        row,
      },
    )?;
  }
  report.record(Disposition::Mapped);
  Ok(Row {
    row_index: Some(u32::from(row) + 1),
    style_index: style.flatten().map(|_| u32::from(style_index)),
    custom_format: bool_attribute(style.flatten().is_some()),
    height: valid_height.then_some(f64::from(source.height) / 20.0),
    hidden: bool_attribute(flags & 0x0000_0020 != 0),
    custom_height: bool_attribute(flags & 0x0000_0040 != 0),
    outline_level: nonzero_u8(u16::try_from(flags & 0x0000_0007).expect("three bits fit u16")),
    collapsed: bool_attribute(flags & 0x0000_0010 != 0),
    thick_top: bool_attribute(flags & 0x1000_0000 != 0),
    thick_bot: bool_attribute(flags & 0x2000_0000 != 0),
    show_phonetic: bool_attribute(flags & 0x4000_0000 != 0),
    cell,
    ..Default::default()
  })
}

const fn bool_attribute(value: bool) -> Option<BooleanValue> {
  if value {
    Some(BooleanValue::True)
  } else {
    None
  }
}

fn nonzero_u8(value: u16) -> Option<u8> {
  (value != 0).then(|| u8::try_from(value).expect("three bits fit u8"))
}

fn convert_shared_strings(
  view: &XlsWorkbookView<'_>,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SharedStringTable>> {
  let Some(source) = view.shared_string_table()? else {
    return Ok(None);
  };
  let mut items = Vec::with_capacity(source.strings.len());
  for (string_index, source_string) in source.strings.iter().enumerate() {
    let string_index = u32::try_from(string_index)
      .map_err(|_| olecfsdk::Error::Limit("XLS SST index exceeds u32".into()))?;
    let source_location = SourceLocation::XlsSharedString {
      workbook_index: 0,
      string_index,
    };
    let (item, rich_text_unmapped, phonetic_text_unmapped, phonetic_compatibility_unmapped) =
      convert_shared_string_item(view, source_string)?;
    if rich_text_unmapped {
      unsupported(
        report,
        options,
        ConversionCode::SharedStringRichTextNotMapped,
        source_location,
      )?;
    }
    if phonetic_text_unmapped {
      unsupported(
        report,
        options,
        ConversionCode::SharedStringPhoneticTextNotMapped,
        source_location,
      )?;
    }
    if phonetic_compatibility_unmapped {
      unsupported(
        report,
        options,
        ConversionCode::SharedStringPhoneticCompatibilityNotMapped,
        source_location,
      )?;
    }
    items.push(item);
    report.record(Disposition::Mapped);
  }
  Ok(Some(SharedStringTable {
    count: Some(source.total_string_count),
    unique_count: Some(source.unique_string_count),
    shared_string_item: items,
    ..Default::default()
  }))
}

fn convert_shared_string_item(
  view: &XlsWorkbookView<'_>,
  source: &SstString,
) -> Result<(SharedStringItem, bool, bool, bool)> {
  let code_units = sst_code_units(source);
  let (mut item, rich_text_unmapped) = if source.format_runs.is_empty() {
    let value = String::from_utf16(&code_units).map_err(|_| {
      olecfsdk::Error::invalid(0, "XLS string contains an unpaired UTF-16 surrogate")
    })?;
    (
      SharedStringItem {
        text: Some(Text(xstring(value))),
        ..Default::default()
      },
      false,
    )
  } else {
    let mut runs = Vec::with_capacity(source.format_runs.len() + 1);
    let mut start = 0usize;
    let mut properties = None;
    let mut unmapped = false;
    for format_run in &source.format_runs {
      let boundary = usize::from(format_run.character_index);
      if boundary > code_units.len() || boundary < start {
        unmapped = true;
        continue;
      }
      if boundary > start {
        runs.push(shared_string_run(
          &code_units[start..boundary],
          properties.clone(),
        )?);
        start = boundary;
      }
      // MS-XLS 2.5.132 says a FormatRun at the string length is undefined
      // and must be ignored.
      if boundary == code_units.len() {
        continue;
      }
      properties = match view.font(format_run.font_index) {
        Some(font) => {
          let (converted, font_unmapped) = convert_run_properties(font)?;
          unmapped |= font_unmapped;
          Some(converted)
        }
        None => {
          unmapped = true;
          None
        }
      };
    }
    if start < code_units.len() {
      runs.push(shared_string_run(&code_units[start..], properties)?);
    }
    (
      SharedStringItem {
        run: runs,
        ..Default::default()
      },
      unmapped,
    )
  };
  let phonetics = convert_phonetic_extension(view, source, code_units.len())?;
  item.phonetic_run = phonetics.runs;
  item.phonetic_properties = phonetics.properties;
  Ok((
    item,
    rich_text_unmapped,
    phonetics.semantic_unmapped,
    phonetics.compatibility_unmapped,
  ))
}

struct ConvertedPhonetics {
  runs: Vec<x::PhoneticRun>,
  properties: Option<x::PhoneticProperties>,
  semantic_unmapped: bool,
  compatibility_unmapped: bool,
}

fn convert_phonetic_extension(
  view: &XlsWorkbookView<'_>,
  source: &SstString,
  base_text_length: usize,
) -> Result<ConvertedPhonetics> {
  // MS-XLS 2.5.109 and 2.5.200 define ExtRst/PhRuns; ECMA-376
  // 18.4.3 and 18.4.6 define the corresponding phoneticPr/rPh values.
  let SstExtensionData::ExtRst(extension) = &source.extension else {
    return Ok(ConvertedPhonetics {
      runs: Vec::new(),
      properties: None,
      semantic_unmapped: !matches!(source.extension, SstExtensionData::None),
      compatibility_unmapped: false,
    });
  };
  let ExtRstBody::Phonetic {
    declared_data_size: _,
    font_index,
    formatting_flags,
    declared_run_count,
    declared_character_count,
    lpwide_character_count,
    phonetic_text,
    runs,
    extra_data_word,
    inner_trailing,
    outer_trailing,
  } = &extension.body
  else {
    return Ok(ConvertedPhonetics {
      runs: Vec::new(),
      properties: None,
      semantic_unmapped: true,
      compatibility_unmapped: false,
    });
  };

  let mut semantic_unmapped = extension.reserved != 1
    || usize::from(*declared_run_count) != runs.len()
    || usize::from(*declared_character_count) != phonetic_text.len()
    || declared_character_count != lpwide_character_count;
  let compatibility_unmapped =
    extra_data_word.is_some() || !inner_trailing.is_empty() || !outer_trailing.is_empty();
  let font_id = match font_position(*font_index).filter(|_| view.font(*font_index).is_some()) {
    Some(position) => u32::try_from(position)
      .map_err(|_| olecfsdk::Error::Limit("XLS phonetic font index exceeds u32".into()))?,
    None => {
      semantic_unmapped = true;
      0
    }
  };
  let properties = x::PhoneticProperties {
    font_id,
    r#type: Some(match formatting_flags.bits() & 0x0003 {
      0 => x::PhoneticValues::HalfWidthKatakana,
      1 => x::PhoneticValues::FullWidthKatakana,
      2 => x::PhoneticValues::Hiragana,
      _ => x::PhoneticValues::NoConversion,
    }),
    alignment: Some(match (formatting_flags.bits() >> 2) & 0x0003 {
      0 => x::PhoneticAlignmentValues::NoControl,
      1 => x::PhoneticAlignmentValues::Left,
      2 => x::PhoneticAlignmentValues::Center,
      _ => x::PhoneticAlignmentValues::Distributed,
    }),
  };

  if phonetic_text.is_empty() {
    return Ok(ConvertedPhonetics {
      runs: Vec::new(),
      properties: Some(properties),
      semantic_unmapped,
      compatibility_unmapped,
    });
  }
  if runs.is_empty() {
    if *declared_run_count != 0 || base_text_length == 0 {
      return Ok(ConvertedPhonetics {
        runs: Vec::new(),
        properties: Some(properties),
        semantic_unmapped: true,
        compatibility_unmapped,
      });
    }
    // MS-XLS 2.5.219 says crun=0 still represents one phonetic run.
    // LibreOffice core's RichString::createPhoneticPortions applies that
    // implicit run to the entire base string.
    let Some(text) = String::from_utf16(phonetic_text).ok() else {
      return Ok(ConvertedPhonetics {
        runs: Vec::new(),
        properties: Some(properties),
        semantic_unmapped: true,
        compatibility_unmapped,
      });
    };
    let ending_base_index = u32::try_from(base_text_length)
      .map_err(|_| olecfsdk::Error::Limit("XLS phonetic base text exceeds u32".into()))?;
    return Ok(ConvertedPhonetics {
      runs: vec![x::PhoneticRun {
        base_text_start_index: 0,
        ending_base_index,
        text: Box::new(Text(xstring(text))),
      }],
      properties: Some(properties),
      semantic_unmapped,
      compatibility_unmapped,
    });
  }

  let mut converted = Vec::with_capacity(runs.len());
  let mut previous_phonetic_start = None;
  let mut previous_source_start = None;
  let mut covered_source_characters = 0usize;
  for (index, run) in runs.iter().enumerate() {
    let phonetic_start = usize::from(run.phonetic_text_first_character);
    let phonetic_end = runs.get(index + 1).map_or(phonetic_text.len(), |next| {
      usize::from(next.phonetic_text_first_character)
    });
    let source_start = usize::from(run.source_text_first_character);
    let source_count = usize::from(run.source_text_character_count);
    let Some(source_end) = source_start.checked_add(source_count) else {
      semantic_unmapped = true;
      continue;
    };
    covered_source_characters = covered_source_characters.saturating_add(source_count);
    if previous_phonetic_start.is_some_and(|previous| previous >= phonetic_start)
      || previous_source_start.is_some_and(|previous| previous >= source_start)
      || phonetic_start >= phonetic_end
      || phonetic_end > phonetic_text.len()
      || source_start >= source_end
      || source_end > base_text_length
    {
      semantic_unmapped = true;
      continue;
    }
    previous_phonetic_start = Some(phonetic_start);
    previous_source_start = Some(source_start);
    let Some(text) = String::from_utf16(&phonetic_text[phonetic_start..phonetic_end]).ok() else {
      semantic_unmapped = true;
      continue;
    };
    converted.push(x::PhoneticRun {
      base_text_start_index: u32::try_from(source_start)
        .map_err(|_| olecfsdk::Error::Limit("XLS phonetic base index exceeds u32".into()))?,
      ending_base_index: u32::try_from(source_end)
        .map_err(|_| olecfsdk::Error::Limit("XLS phonetic base index exceeds u32".into()))?,
      text: Box::new(Text(xstring(text))),
    });
  }
  if covered_source_characters > base_text_length {
    semantic_unmapped = true;
  }
  Ok(ConvertedPhonetics {
    runs: converted,
    properties: Some(properties),
    semantic_unmapped,
    compatibility_unmapped,
  })
}

fn sst_code_units(source: &SstString) -> Vec<u16> {
  let mut code_units = Vec::with_capacity(usize::from(source.declared_character_count));
  for chunk in &source.character_chunks {
    match &chunk.characters {
      XlStringCharacters::Compressed(bytes) => {
        code_units.extend(bytes.iter().copied().map(u16::from));
      }
      XlStringCharacters::Unicode(units) => code_units.extend_from_slice(units),
    }
  }
  code_units
}

fn shared_string_run(
  code_units: &[u16],
  run_properties: Option<x::RunProperties>,
) -> Result<x::Run> {
  let value = String::from_utf16(code_units)
    .map_err(|_| olecfsdk::Error::invalid(0, "XLS string contains an unpaired UTF-16 surrogate"))?;
  Ok(x::Run {
    run_properties,
    text: Box::new(Text(xstring(value))),
  })
}

fn convert_run_properties(source: &FontRecord) -> Result<(x::RunProperties, bool)> {
  let (font, unmapped) = convert_font(source, None)?;
  let run_properties_choice = font
    .font_choice
    .into_iter()
    .map(|choice| match choice {
      x::FontChoice::Bold(value) => x::RunPropertiesChoice::Bold(value),
      x::FontChoice::Italic(value) => x::RunPropertiesChoice::Italic(value),
      x::FontChoice::Strike(value) => x::RunPropertiesChoice::Strike(value),
      x::FontChoice::Condense(value) => x::RunPropertiesChoice::Condense(value),
      x::FontChoice::Extend(value) => x::RunPropertiesChoice::Extend(value),
      x::FontChoice::Outline(value) => x::RunPropertiesChoice::Outline(value),
      x::FontChoice::Shadow(value) => x::RunPropertiesChoice::Shadow(value),
      x::FontChoice::Underline(value) => x::RunPropertiesChoice::Underline(value),
      x::FontChoice::VerticalTextAlignment(value) => {
        x::RunPropertiesChoice::VerticalTextAlignment(value)
      }
      x::FontChoice::FontSize(value) => x::RunPropertiesChoice::FontSize(value),
      x::FontChoice::Color(value) => x::RunPropertiesChoice::Color(value),
      x::FontChoice::FontName(value) => {
        x::RunPropertiesChoice::RunFont(x::RunFont { val: value.val })
      }
      x::FontChoice::FontFamilyNumbering(value) => {
        x::RunPropertiesChoice::FontFamily(x::FontFamily { val: value.val })
      }
      x::FontChoice::FontCharSet(value) => {
        x::RunPropertiesChoice::RunPropertyCharSet(x::RunPropertyCharSet { val: value.val })
      }
      x::FontChoice::FontScheme(value) => x::RunPropertiesChoice::FontScheme(value),
    })
    .collect();
  Ok((
    x::RunProperties {
      run_properties_choice,
    },
    unmapped,
  ))
}

struct ConvertedStylesheet {
  root: x::Stylesheet,
  xf_unmapped: Vec<bool>,
}

fn convert_stylesheet(view: &XlsWorkbookView<'_>) -> Result<ConvertedStylesheet> {
  let source_fonts = view.fonts().collect::<Vec<_>>();
  let mut fonts = Vec::with_capacity(source_fonts.len());
  let mut font_unmapped = Vec::with_capacity(source_fonts.len());
  for source in source_fonts {
    let (font, unmapped) = convert_font(source, None)?;
    fonts.push(x::FontsChoice::Font(font));
    font_unmapped.push(unmapped);
  }
  if fonts.is_empty() {
    fonts.push(x::FontsChoice::Font(x::Font::default()));
    font_unmapped.push(false);
  }

  let xfs = view.xfs().collect::<Vec<_>>();
  let mut extensions = vec![None; xfs.len()];
  let mut duplicate_extension = vec![false; xfs.len()];
  for extension in view.xf_extensions() {
    let index = usize::from(extension.xf_index);
    if let Some(slot) = extensions.get_mut(index)
      && slot.replace(extension).is_some()
    {
      duplicate_extension[index] = true;
    }
  }
  let mut style_positions = vec![None; xfs.len()];
  let mut next_style = 0u32;
  for (index, xf) in xfs.iter().enumerate() {
    if xf.cell_flags & 0x0004 != 0 {
      style_positions[index] = Some(next_style);
      next_style = next_style
        .checked_add(1)
        .ok_or_else(|| olecfsdk::Error::Limit("XLS style XF count exceeds u32".into()))?;
    }
  }
  let mut cell_formats = Vec::with_capacity(xfs.len().max(1));
  let mut style_formats = Vec::with_capacity(xfs.len());
  let mut fills = Vec::with_capacity(xfs.len() + 2);
  fills.push(x::FillsChoice::Fill(Box::new(pattern_fill(
    x::PatternValues::None,
    0,
    0,
    None,
  ))));
  fills.push(x::FillsChoice::Fill(Box::new(pattern_fill(
    x::PatternValues::Gray125,
    0,
    0,
    None,
  ))));
  let mut borders = Vec::with_capacity(xfs.len().max(1));
  let mut xf_unmapped = Vec::with_capacity(xfs.len());
  for (index, xf) in xfs.iter().copied().enumerate() {
    let extension = extensions[index];
    let source_font_index = font_position(xf.font_index);
    let (target_font_id, mut unmapped) = match source_font_index {
      Some(source_font_index) => {
        let source_font_unmapped = font_unmapped
          .get(source_font_index)
          .copied()
          .unwrap_or(true);
        if extension.is_some_and(has_font_extension) {
          let source_font = view.font(xf.font_index);
          if let Some(source_font) = source_font {
            let (font, derived_unmapped) = convert_font(source_font, extension)?;
            let target_font_id = u32::try_from(fonts.len())
              .map_err(|_| olecfsdk::Error::Limit("XLS target font count exceeds u32".into()))?;
            fonts.push(x::FontsChoice::Font(font));
            (target_font_id, source_font_unmapped | derived_unmapped)
          } else {
            (0, true)
          }
        } else {
          (
            u32::try_from(source_font_index)
              .map_err(|_| olecfsdk::Error::Limit("XLS font index exceeds u32".into()))?,
            source_font_unmapped,
          )
        }
      }
      None => (0, true),
    };
    unmapped |= xf_has_unmapped_base(xf, &style_positions)
      || duplicate_extension[index]
      || extension.is_some() != (xf.additional_border_color_flags & 0x0200_0000 != 0)
      || extension.is_some_and(extension_has_unmapped_property);
    let format = convert_xf(xf, index, &style_positions, target_font_id, extension);
    let pattern =
      u8::try_from((xf.additional_border_color_flags >> 26) & 0x3f).expect("six bits fit u8");
    let pattern_type = fill_pattern(pattern);
    if pattern_type.is_none() {
      unmapped = true;
    }
    fills.push(x::FillsChoice::Fill(Box::new(pattern_fill(
      pattern_type.unwrap_or_default(),
      u32::from(xf.fill_flags & 0x007f),
      u32::from((xf.fill_flags >> 7) & 0x007f),
      extension,
    ))));
    let (border, border_unmapped) = convert_border(xf, extension);
    borders.push(border);
    unmapped |= border_unmapped;
    if xf.cell_flags & 0x0004 != 0 {
      style_formats.push(convert_xf(
        xf,
        index,
        &style_positions,
        target_font_id,
        extension,
      ));
    }
    cell_formats.push(x::CellFormatsChoice::CellFormat(Box::new(format)));
    xf_unmapped.push(unmapped);
  }
  if cell_formats.is_empty() {
    cell_formats.push(x::CellFormatsChoice::CellFormat(Box::default()));
    xf_unmapped.push(false);
    borders.push(Default::default());
  }
  if style_formats.is_empty() {
    style_formats.push(Default::default());
  }

  let numbering_format = view
    .formats()
    .map(|format| {
      Ok(x::NumberingFormat {
        number_format_id: u32::from(format.format_index),
        format_code: String::try_from(&format.format_string)?,
        ..Default::default()
      })
    })
    .collect::<Result<Vec<_>>>()?;
  let count = |value: usize, label: &str| {
    u32::try_from(value)
      .map_err(|_| olecfsdk::Error::Limit(format!("XLS {label} count exceeds u32")))
  };
  Ok(ConvertedStylesheet {
    root: x::Stylesheet {
      numbering_formats: (!numbering_format.is_empty()).then_some(x::NumberingFormats {
        count: Some(count(numbering_format.len(), "number format")?),
        numbering_format,
      }),
      fonts: Some(x::Fonts {
        count: Some(count(fonts.len(), "font")?),
        known_fonts: None,
        xml_children: fonts,
      }),
      fills: Some(x::Fills {
        count: Some(count(fills.len(), "fill")?),
        xml_children: fills,
      }),
      borders: Some(x::Borders {
        count: Some(count(borders.len(), "border")?),
        border: borders,
      }),
      cell_style_formats: Some(x::CellStyleFormats {
        count: Some(count(style_formats.len(), "style XF")?),
        cell_format: style_formats,
      }),
      cell_formats: Some(x::CellFormats {
        count: Some(count(cell_formats.len(), "cell XF")?),
        xml_children: cell_formats,
      }),
      cell_styles: Some(x::CellStyles {
        count: Some(1),
        cell_style: vec![x::CellStyle {
          name: Some("Normal".into()),
          format_id: 0,
          builtin_id: Some(0),
          ..Default::default()
        }],
      }),
      ..Default::default()
    },
    xf_unmapped,
  })
}

const fn font_position(index: u16) -> Option<usize> {
  match index {
    0..=3 => Some(index as usize),
    4 => None,
    _ => Some((index - 1) as usize),
  }
}

fn xf_has_unmapped_base(source: &XfRecord, style_positions: &[Option<u32>]) -> bool {
  let style = source.cell_flags & 0x0004 != 0;
  let parent = usize::from(source.cell_flags >> 4);
  (!style && parent != 0x0fff && style_positions.get(parent).copied().flatten().is_none())
    || source.indentation_flags & 0x0320 != 0
    || source.fill_flags & 0x8000 != 0
    || horizontal_alignment(source.alignment_flags & 0x0007).is_none()
      && source.alignment_flags & 0x0007 != 0
    || vertical_alignment((source.alignment_flags >> 4) & 0x0007).is_none()
}

fn has_font_extension(extension: &XfExtRecord) -> bool {
  extension.properties.iter().any(|property| {
    matches!(
      property.data,
      ExtPropertyData::FullColor {
        property_type: 0x000d,
        ..
      } | ExtPropertyData::FontScheme(_)
    )
  })
}

fn extension_font_scheme(extension: &XfExtRecord) -> Option<u16> {
  extension
    .properties
    .iter()
    .rev()
    .find_map(|property| match property.data {
      ExtPropertyData::FontScheme(ExtFontScheme::Byte(value)) => Some(u16::from(value)),
      ExtPropertyData::FontScheme(ExtFontScheme::Word(value)) => Some(value),
      _ => None,
    })
}

fn extension_indentation(extension: &XfExtRecord) -> Option<u16> {
  extension
    .properties
    .iter()
    .rev()
    .find_map(|property| match property.data {
      ExtPropertyData::Indentation(value) => Some(value),
      _ => None,
    })
}

fn extension_property_type(property: &ExtPropertyData) -> u16 {
  match property {
    ExtPropertyData::FullColor { property_type, .. }
    | ExtPropertyData::Unknown { property_type, .. } => *property_type,
    ExtPropertyData::Gradient { .. } => 0x0006,
    ExtPropertyData::FontScheme(_) => 0x000e,
    ExtPropertyData::Indentation(_) => 0x000f,
  }
}

fn extension_has_unmapped_property(extension: &XfExtRecord) -> bool {
  extension
    .properties
    .iter()
    .enumerate()
    .any(|(index, property)| {
      let property_type = extension_property_type(&property.data);
      extension.properties[..index]
        .iter()
        .any(|previous| extension_property_type(&previous.data) == property_type)
        || match &property.data {
          ExtPropertyData::FullColor {
            property_type: 0x0004 | 0x0005 | 0x0007 | 0x0008 | 0x0009 | 0x000a | 0x000b | 0x000d,
            color,
          } => convert_full_color(color).1,
          ExtPropertyData::FontScheme(value) => !matches!(
            value,
            ExtFontScheme::Byte(0..=2) | ExtFontScheme::Word(0..=2)
          ),
          ExtPropertyData::Indentation(value) => *value > 250,
          ExtPropertyData::FullColor { .. }
          | ExtPropertyData::Gradient { .. }
          | ExtPropertyData::Unknown { .. } => true,
        }
    })
}

fn extension_color(extension: &XfExtRecord, target_type: u16) -> Option<x::Color> {
  extension
    .properties
    .iter()
    .rev()
    .find_map(|property| match &property.data {
      ExtPropertyData::FullColor {
        property_type,
        color,
      } if *property_type == target_type => convert_full_color(color).0,
      _ => None,
    })
}

fn convert_full_color(source: &FullColorExt) -> (Option<x::Color>, bool) {
  let tint = if source.tint < 0 {
    f64::from(source.tint) / 32_768.0
  } else {
    f64::from(source.tint) / 32_767.0
  };
  let tint = (source.tint != 0).then_some(tint);
  let mut color = x::Color {
    tint,
    ..Default::default()
  };
  match source.color_type {
    0 => color.auto = Some(BooleanValue::True),
    1 => color.indexed = Some(source.color_value),
    2 => {
      let [red, green, blue, alpha] = source.color_value.to_le_bytes();
      color.rgb = Some(format!("{alpha:02X}{red:02X}{green:02X}{blue:02X}"));
    }
    3 => color.theme = Some(source.color_value),
    4 => return (None, source.color_value != 0 || source.tint != 0),
    _ => return (None, true),
  }
  (Some(color), false)
}

fn convert_font(source: &FontRecord, extension: Option<&XfExtRecord>) -> Result<(x::Font, bool)> {
  let mut choices = Vec::new();
  let mut unmapped = source.attributes.bits()
    & !(FontAttributes::ITALIC
      | FontAttributes::STRIKEOUT
      | FontAttributes::MAC_OUTLINE
      | FontAttributes::MAC_SHADOW
      | FontAttributes::CONDENSE
      | FontAttributes::EXTEND)
      .bits()
    & !0xff05
    != 0;
  if source.bold_weight >= 700 {
    choices.push(x::FontChoice::Bold(Default::default()));
  }
  unmapped |= !matches!(source.bold_weight, 400 | 700);
  if source.attributes.contains(FontAttributes::ITALIC) {
    choices.push(x::FontChoice::Italic(Default::default()));
  }
  if source.attributes.contains(FontAttributes::STRIKEOUT) {
    choices.push(x::FontChoice::Strike(Default::default()));
  }
  if source.attributes.contains(FontAttributes::MAC_OUTLINE) {
    choices.push(x::FontChoice::Outline(Default::default()));
  }
  if source.attributes.contains(FontAttributes::MAC_SHADOW) {
    choices.push(x::FontChoice::Shadow(Default::default()));
  }
  if source.attributes.contains(FontAttributes::CONDENSE) {
    choices.push(x::FontChoice::Condense(x::Condense {
      val: Some(BooleanValue::True),
    }));
  }
  if source.attributes.contains(FontAttributes::EXTEND) {
    choices.push(x::FontChoice::Extend(x::Extend {
      val: Some(BooleanValue::True),
    }));
  }
  if let Some(value) = font_underline(source.underline) {
    choices.push(x::FontChoice::Underline(x::Underline { val: Some(value) }));
  } else if source.underline != 0 {
    unmapped = true;
  }
  if let Some(value) = font_vertical_alignment(source.escapement) {
    choices.push(x::FontChoice::VerticalTextAlignment(
      x::VerticalTextAlignment { val: value },
    ));
  } else if source.escapement != 0 {
    unmapped = true;
  }
  choices.push(x::FontChoice::FontSize(x::FontSize {
    val: f64::from(source.height_twips) / 20.0,
  }));
  let extended_color = extension.and_then(|extension| {
    extension.properties.iter().rev().find_map(|property| {
      let ExtPropertyData::FullColor {
        property_type: 0x000d,
        color,
      } = &property.data
      else {
        return None;
      };
      Some(color)
    })
  });
  let color = if let Some(color) = extended_color {
    let (color, color_unmapped) = convert_full_color(color);
    unmapped |= color_unmapped;
    color.unwrap_or_default()
  } else {
    x::Color {
      auto: (source.color_index == 0x7fff).then_some(BooleanValue::True),
      indexed: (source.color_index != 0x7fff).then_some(u32::from(source.color_index)),
      ..Default::default()
    }
  };
  choices.push(x::FontChoice::Color(color));
  let name = String::try_from(&source.name)?;
  if name.is_empty() {
    unmapped = true;
  } else {
    choices.push(x::FontChoice::FontName(x::FontName { val: name }));
  }
  if source.family <= 5 {
    choices.push(x::FontChoice::FontFamilyNumbering(x::FontFamilyNumbering {
      val: i32::from(source.family),
    }));
  } else {
    unmapped = true;
  }
  choices.push(x::FontChoice::FontCharSet(x::FontCharSet {
    val: i32::from(source.charset),
  }));
  if let Some(scheme) = extension.and_then(extension_font_scheme) {
    let scheme = match scheme {
      0 => x::FontSchemeValues::None,
      1 => x::FontSchemeValues::Major,
      2 => x::FontSchemeValues::Minor,
      _ => {
        unmapped = true;
        x::FontSchemeValues::None
      }
    };
    choices.push(x::FontChoice::FontScheme(x::FontScheme { val: scheme }));
  }
  Ok((
    x::Font {
      font_choice: choices,
    },
    unmapped,
  ))
}

fn convert_xf(
  source: &XfRecord,
  xf_index: usize,
  style_positions: &[Option<u32>],
  target_font_id: u32,
  extension: Option<&XfExtRecord>,
) -> x::CellFormat {
  let style = source.cell_flags & 0x0004 != 0;
  let parent = usize::from(source.cell_flags >> 4);
  let format_id = (!style)
    .then(|| style_positions.get(parent).copied().flatten())
    .flatten();
  let horizontal = horizontal_alignment(source.alignment_flags & 0x0007);
  let vertical = vertical_alignment((source.alignment_flags >> 4) & 0x0007);
  let alignment = x::Alignment {
    horizontal,
    vertical,
    text_rotation: Some(u32::from(source.alignment_flags >> 8)),
    wrap_text: Some(BooleanValue::from_bool(
      source.alignment_flags & 0x0008 != 0,
    )),
    indent: Some(u32::from(
      extension
        .and_then(extension_indentation)
        .unwrap_or(source.indentation_flags & 0x000f),
    )),
    justify_last_line: Some(BooleanValue::from_bool(
      source.alignment_flags & 0x0080 != 0,
    )),
    shrink_to_fit: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x0010 != 0,
    )),
    reading_order: Some(u32::from((source.indentation_flags >> 6) & 0x0003)),
    ..Default::default()
  };
  let fill_id = u32::try_from(xf_index + 2).unwrap_or(u32::MAX);
  let border_id = u32::try_from(xf_index).unwrap_or(u32::MAX);
  x::CellFormat {
    number_format_id: Some(u32::from(source.number_format_index)),
    font_id: Some(target_font_id),
    fill_id: Some(fill_id),
    border_id: Some(border_id),
    format_id: format_id.or(Some(0)),
    quote_prefix: Some(BooleanValue::from_bool(source.cell_flags & 0x0008 != 0)),
    pivot_button: Some(BooleanValue::from_bool(source.fill_flags & 0x4000 != 0)),
    apply_number_format: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x0400 != 0,
    )),
    apply_font: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x0800 != 0,
    )),
    apply_alignment: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x1000 != 0,
    )),
    apply_border: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x2000 != 0,
    )),
    apply_fill: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x4000 != 0,
    )),
    apply_protection: Some(BooleanValue::from_bool(
      source.indentation_flags & 0x8000 != 0,
    )),
    alignment: Some(alignment),
    protection: Some(x::Protection {
      locked: Some(BooleanValue::from_bool(source.cell_flags & 0x0001 != 0)),
      hidden: Some(BooleanValue::from_bool(source.cell_flags & 0x0002 != 0)),
    }),
    ..Default::default()
  }
}

fn pattern_fill(
  pattern: x::PatternValues,
  foreground: u32,
  background: u32,
  extension: Option<&XfExtRecord>,
) -> x::Fill {
  let extended_foreground = extension.and_then(|extension| extension_color(extension, 0x0004));
  let extended_background = extension.and_then(|extension| extension_color(extension, 0x0005));
  let x::Color {
    auto: foreground_auto,
    indexed: foreground_indexed,
    rgb: foreground_rgb,
    theme: foreground_theme,
    tint: foreground_tint,
  } = extended_foreground.unwrap_or(x::Color {
    indexed: Some(foreground),
    ..Default::default()
  });
  let x::Color {
    auto: background_auto,
    indexed: background_indexed,
    rgb: background_rgb,
    theme: background_theme,
    tint: background_tint,
  } = extended_background.unwrap_or(x::Color {
    indexed: Some(background),
    ..Default::default()
  });
  x::Fill {
    fill_choice: Some(x::FillChoice::PatternFill(Box::new(x::PatternFill {
      pattern_type: Some(pattern),
      foreground_color: Some(x::ForegroundColor {
        auto: foreground_auto,
        indexed: foreground_indexed,
        rgb: foreground_rgb,
        theme: foreground_theme,
        tint: foreground_tint,
      }),
      background_color: Some(x::BackgroundColor {
        auto: background_auto,
        indexed: background_indexed,
        rgb: background_rgb,
        theme: background_theme,
        tint: background_tint,
      }),
    }))),
  }
}

fn convert_border(source: &XfRecord, extension: Option<&XfExtRecord>) -> (x::Border, bool) {
  let style = |value| border_style(value);
  let left_style = style(source.border_style_flags & 0x000f);
  let right_style = style((source.border_style_flags >> 4) & 0x000f);
  let top_style = style((source.border_style_flags >> 8) & 0x000f);
  let bottom_style = style((source.border_style_flags >> 12) & 0x000f);
  let diagonal_style = style(
    u16::try_from((source.additional_border_color_flags >> 21) & 0x000f)
      .expect("four bits fit u16"),
  );
  let diagonal = (source.border_color_flags >> 14) & 0x0003;
  let border = x::Border {
    diagonal_down: Some(BooleanValue::from_bool(diagonal & 1 != 0)),
    diagonal_up: Some(BooleanValue::from_bool(diagonal & 2 != 0)),
    left_border: Some(Box::new(x::LeftBorder {
      style: left_style,
      color: extension
        .and_then(|extension| extension_color(extension, 0x0009))
        .or_else(|| border_color(u32::from(source.border_color_flags & 0x007f))),
    })),
    right_border: Some(Box::new(x::RightBorder {
      style: right_style,
      color: extension
        .and_then(|extension| extension_color(extension, 0x000a))
        .or_else(|| border_color(u32::from((source.border_color_flags >> 7) & 0x007f))),
    })),
    top_border: Some(Box::new(x::TopBorder {
      style: top_style,
      color: extension
        .and_then(|extension| extension_color(extension, 0x0007))
        .or_else(|| border_color(source.additional_border_color_flags & 0x007f)),
    })),
    bottom_border: Some(Box::new(x::BottomBorder {
      style: bottom_style,
      color: extension
        .and_then(|extension| extension_color(extension, 0x0008))
        .or_else(|| border_color((source.additional_border_color_flags >> 7) & 0x007f)),
    })),
    diagonal_border: Some(Box::new(x::DiagonalBorder {
      style: diagonal_style,
      color: extension
        .and_then(|extension| extension_color(extension, 0x000b))
        .or_else(|| border_color((source.additional_border_color_flags >> 14) & 0x007f)),
    })),
    ..Default::default()
  };
  (
    border,
    [
      left_style,
      right_style,
      top_style,
      bottom_style,
      diagonal_style,
    ]
    .iter()
    .enumerate()
    .any(|(index, value)| {
      value.is_none()
        && [
          source.border_style_flags & 0x000f,
          (source.border_style_flags >> 4) & 0x000f,
          (source.border_style_flags >> 8) & 0x000f,
          (source.border_style_flags >> 12) & 0x000f,
          ((source.additional_border_color_flags >> 21) & 0x000f) as u16,
        ][index]
          != 0
    }),
  )
}

fn border_color(index: u32) -> Option<x::Color> {
  (index != 0).then_some(x::Color {
    indexed: Some(index),
    ..Default::default()
  })
}

const fn border_style(value: u16) -> Option<x::BorderStyleValues> {
  Some(match value {
    0 => return None,
    1 => x::BorderStyleValues::Thin,
    2 => x::BorderStyleValues::Medium,
    3 => x::BorderStyleValues::Dashed,
    4 => x::BorderStyleValues::Dotted,
    5 => x::BorderStyleValues::Thick,
    6 => x::BorderStyleValues::Double,
    7 => x::BorderStyleValues::Hair,
    8 => x::BorderStyleValues::MediumDashed,
    9 => x::BorderStyleValues::DashDot,
    10 => x::BorderStyleValues::MediumDashDot,
    11 => x::BorderStyleValues::DashDotDot,
    12 => x::BorderStyleValues::MediumDashDotDot,
    13 => x::BorderStyleValues::SlantDashDot,
    _ => return None,
  })
}

const fn fill_pattern(value: u8) -> Option<x::PatternValues> {
  Some(match value {
    0 => x::PatternValues::None,
    1 => x::PatternValues::Solid,
    2 => x::PatternValues::MediumGray,
    3 => x::PatternValues::DarkGray,
    4 => x::PatternValues::LightGray,
    5 => x::PatternValues::DarkHorizontal,
    6 => x::PatternValues::DarkVertical,
    7 => x::PatternValues::DarkDown,
    8 => x::PatternValues::DarkUp,
    9 => x::PatternValues::DarkGrid,
    10 => x::PatternValues::DarkTrellis,
    11 => x::PatternValues::LightHorizontal,
    12 => x::PatternValues::LightVertical,
    13 => x::PatternValues::LightDown,
    14 => x::PatternValues::LightUp,
    15 => x::PatternValues::LightGrid,
    16 => x::PatternValues::LightTrellis,
    17 => x::PatternValues::Gray125,
    18 => x::PatternValues::Gray0625,
    _ => return None,
  })
}

const fn horizontal_alignment(value: u16) -> Option<x::HorizontalAlignmentValues> {
  Some(match value {
    0 => return None,
    1 => x::HorizontalAlignmentValues::Left,
    2 => x::HorizontalAlignmentValues::Center,
    3 => x::HorizontalAlignmentValues::Right,
    4 => x::HorizontalAlignmentValues::Fill,
    5 => x::HorizontalAlignmentValues::Justify,
    6 => x::HorizontalAlignmentValues::CenterContinuous,
    7 => x::HorizontalAlignmentValues::Distributed,
    _ => return None,
  })
}

const fn vertical_alignment(value: u16) -> Option<x::VerticalAlignmentValues> {
  Some(match value {
    0 => x::VerticalAlignmentValues::Top,
    1 => x::VerticalAlignmentValues::Center,
    2 => x::VerticalAlignmentValues::Bottom,
    3 => x::VerticalAlignmentValues::Justify,
    4 => x::VerticalAlignmentValues::Distributed,
    _ => return None,
  })
}

const fn font_underline(value: u8) -> Option<x::UnderlineValues> {
  Some(match value {
    0 => return None,
    1 => x::UnderlineValues::Single,
    2 => x::UnderlineValues::Double,
    0x21 => x::UnderlineValues::SingleAccounting,
    0x22 => x::UnderlineValues::DoubleAccounting,
    _ => return None,
  })
}

const fn font_vertical_alignment(value: u16) -> Option<x::VerticalAlignmentRunValues> {
  Some(match value {
    0 => return None,
    1 => x::VerticalAlignmentRunValues::Superscript,
    2 => x::VerticalAlignmentRunValues::Subscript,
    _ => return None,
  })
}

fn render_cell_formula(formula: XlsFormulaRef<'_>, row: u16, column: u16) -> Option<CellFormula> {
  if formula.formula().flags & 0x0020 != 0 {
    return None;
  }
  let (tokens, shared) = match formula.definition() {
    XlsFormulaDefinitionRef::Inline(tokens) => (tokens, false),
    XlsFormulaDefinitionRef::Shared(formula) => (&formula.tokens, true),
    XlsFormulaDefinitionRef::Array(_)
    | XlsFormulaDefinitionRef::Table(_)
    | XlsFormulaDefinitionRef::UnresolvedExp { .. }
    | XlsFormulaDefinitionRef::UnresolvedTable { .. } => return None,
  };
  if !tokens.rgcb_tail.is_empty() {
    return None;
  }
  Some(CellFormula {
    calculate_cell: (formula.formula().flags & 0x0001 != 0).then_some(BooleanValue::True),
    xml_content: render_formula_tokens(&tokens.rgce, row, column, shared),
    ..Default::default()
  })
  .filter(|formula| formula.xml_content.is_some())
}

fn render_formula_tokens(
  source: &FormulaTokenStream,
  row: u16,
  column: u16,
  shared: bool,
) -> Option<String> {
  if !source.unparsed_tail.is_empty()
    || source.missing_extra_count() != 0
    || source.nonconforming_token_count() != 0
  {
    return None;
  }
  let mut stack = Vec::with_capacity(source.tokens.len());
  for token in &source.tokens {
    match &token.data {
      FormulaTokenData::Operator(operator) => {
        apply_formula_operator(&mut stack, *operator)?;
      }
      FormulaTokenData::String { flags, characters } if flags & !1 == 0 => {
        let value = formula_string(characters)?;
        stack.push(format!("\"{}\"", value.replace('"', "\"\"")));
      }
      FormulaTokenData::Error(value) => stack.push(formula_error(*value)?.into()),
      FormulaTokenData::Boolean(0) => stack.push("FALSE".into()),
      FormulaTokenData::Boolean(1) => stack.push("TRUE".into()),
      FormulaTokenData::Integer(value) => stack.push(value.to_string()),
      FormulaTokenData::NumberBits(bits) => {
        let value = f64::from_bits(*bits);
        if !value.is_finite() {
          return None;
        }
        stack.push(value.to_string());
      }
      FormulaTokenData::Reference {
        row: target_row,
        column: target_column,
      } => stack.push(formula_reference(
        *target_row,
        *target_column,
        row,
        column,
        false,
      )?),
      FormulaTokenData::RelativeReference {
        row: target_row,
        column: target_column,
      } => stack.push(formula_reference(
        *target_row,
        *target_column,
        row,
        column,
        shared,
      )?),
      FormulaTokenData::Area {
        first_row,
        last_row,
        first_column,
        last_column,
      } => stack.push(formula_area(
        (*first_row, *first_column),
        (*last_row, *last_column),
        row,
        column,
        false,
      )?),
      FormulaTokenData::RelativeArea {
        first_row,
        last_row,
        first_column,
        last_column,
      } => stack.push(formula_area(
        (*first_row, *first_column),
        (*last_row, *last_column),
        row,
        column,
        shared,
      )?),
      FormulaTokenData::ReferenceError { .. } | FormulaTokenData::AreaError { .. } => {
        stack.push("#REF!".into());
      }
      FormulaTokenData::Function { function_index } => {
        let (name, argument_count) = fixed_function(*function_index)?;
        apply_formula_function(&mut stack, name, argument_count)?;
      }
      FormulaTokenData::FunctionVar {
        argument_count,
        function_index,
      } if function_index & 0x8000 == 0 => {
        apply_formula_function(
          &mut stack,
          formula_function_name(*function_index)?,
          usize::from(*argument_count),
        )?;
      }
      FormulaTokenData::Attribute { options, .. } if options & 0x1e == 0x10 => {
        apply_formula_function(&mut stack, "SUM", 1)?;
      }
      FormulaTokenData::Attribute { options, .. } if options & 0x1e == 0 => {}
      FormulaTokenData::MemArea { .. }
      | FormulaTokenData::MemNoMem { .. }
      | FormulaTokenData::MemFunction { .. } => {}
      FormulaTokenData::UnknownZero
      | FormulaTokenData::Exp { .. }
      | FormulaTokenData::Table { .. }
      | FormulaTokenData::String { .. }
      | FormulaTokenData::PivotName { .. }
      | FormulaTokenData::NaturalLanguage { .. }
      | FormulaTokenData::Attribute { .. }
      | FormulaTokenData::Boolean(_)
      | FormulaTokenData::Array { .. }
      | FormulaTokenData::Name { .. }
      | FormulaTokenData::MemError { .. }
      | FormulaTokenData::ExternalName { .. }
      | FormulaTokenData::Reference3d { .. }
      | FormulaTokenData::Area3d { .. }
      | FormulaTokenData::DeletedReference3d { .. }
      | FormulaTokenData::DeletedArea3d { .. }
      | FormulaTokenData::FunctionVar { .. } => return None,
    }
  }
  let mut expression = stack.pop()?;
  if !stack.is_empty() {
    return None;
  }
  if expression.starts_with('(') && expression.ends_with(')') {
    expression.remove(0);
    expression.pop();
  }
  Some(expression)
}

fn apply_formula_operator(stack: &mut Vec<String>, operator: FormulaOperator) -> Option<()> {
  if operator == FormulaOperator::MissingArgument {
    stack.push(String::new());
    return Some(());
  }
  let binary = match operator {
    FormulaOperator::Add => Some("+"),
    FormulaOperator::Subtract => Some("-"),
    FormulaOperator::Multiply => Some("*"),
    FormulaOperator::Divide => Some("/"),
    FormulaOperator::Power => Some("^"),
    FormulaOperator::Concat => Some("&"),
    FormulaOperator::LessThan => Some("<"),
    FormulaOperator::LessEqual => Some("<="),
    FormulaOperator::Equal => Some("="),
    FormulaOperator::GreaterEqual => Some(">="),
    FormulaOperator::GreaterThan => Some(">"),
    FormulaOperator::NotEqual => Some("<>"),
    FormulaOperator::Intersection => Some(" "),
    FormulaOperator::Union => Some(","),
    FormulaOperator::Range => Some(":"),
    _ => None,
  };
  if let Some(operator) = binary {
    let right = stack.pop()?;
    let left = stack.pop()?;
    stack.push(format!("({left}{operator}{right})"));
    return Some(());
  }
  let value = stack.pop()?;
  stack.push(match operator {
    FormulaOperator::UnaryPlus => format!("(+{value})"),
    FormulaOperator::UnaryMinus => format!("(-{value})"),
    FormulaOperator::Percent => format!("({value}%)"),
    FormulaOperator::Parenthesis => format!("({value})"),
    FormulaOperator::MissingArgument => unreachable!("handled before popping an operand"),
    _ => return None,
  });
  Some(())
}

fn apply_formula_function(
  stack: &mut Vec<String>,
  name: &str,
  argument_count: usize,
) -> Option<()> {
  let first = stack.len().checked_sub(argument_count)?;
  let arguments = stack.drain(first..).collect::<Vec<_>>().join(",");
  stack.push(format!("{name}({arguments})"));
  Some(())
}

fn formula_string(value: &XlStringCharacters) -> Option<String> {
  match value {
    XlStringCharacters::Compressed(value) => Some(value.iter().copied().map(char::from).collect()),
    XlStringCharacters::Unicode(value) => String::from_utf16(value).ok(),
  }
}

fn formula_reference(
  source_row: u16,
  source_column: u16,
  formula_row: u16,
  formula_column: u16,
  offsets: bool,
) -> Option<String> {
  let column_relative = source_column & 0x4000 != 0;
  let row_relative = source_column & 0x8000 != 0;
  let raw_column = source_column & 0x3fff;
  let row = if offsets && row_relative {
    formula_row.wrapping_add(source_row)
  } else {
    source_row
  };
  let column = if offsets && column_relative {
    let offset = i32::from(((raw_column << 2) as i16) >> 2);
    u16::try_from((i32::from(formula_column) + offset).rem_euclid(256)).ok()?
  } else {
    raw_column
  };
  if column > 255 {
    return None;
  }
  let mut reference = String::new();
  if !column_relative {
    reference.push('$');
  }
  append_column_name(&mut reference, column);
  if !row_relative {
    reference.push('$');
  }
  reference.push_str(&(u32::from(row) + 1).to_string());
  Some(reference)
}

fn formula_area(
  first: (u16, u16),
  last: (u16, u16),
  formula_row: u16,
  formula_column: u16,
  offsets: bool,
) -> Option<String> {
  Some(format!(
    "{}:{}",
    formula_reference(first.0, first.1, formula_row, formula_column, offsets)?,
    formula_reference(last.0, last.1, formula_row, formula_column, offsets)?
  ))
}

fn append_column_name(target: &mut String, column: u16) {
  let mut value = u32::from(column) + 1;
  let mut letters = [0u8; 3];
  let mut start = letters.len();
  while value != 0 {
    value -= 1;
    start -= 1;
    letters[start] = b'A' + u8::try_from(value % 26).expect("modulo 26 fits u8");
    value /= 26;
  }
  target.push_str(std::str::from_utf8(&letters[start..]).expect("ASCII column name"));
}

const fn formula_error(value: u8) -> Option<&'static str> {
  Some(match value {
    0x00 => "#NULL!",
    0x07 => "#DIV/0!",
    0x0f => "#VALUE!",
    0x17 => "#REF!",
    0x1d => "#NAME?",
    0x24 => "#NUM!",
    0x2a => "#N/A",
    0x2b => "#GETTING_DATA",
    _ => return None,
  })
}

const fn fixed_function(index: u16) -> Option<(&'static str, usize)> {
  Some(match index {
    2 => ("ISNA", 1),
    3 => ("ISERROR", 1),
    8 => ("ROW", 0),
    9 => ("COLUMN", 0),
    10 => ("NA", 0),
    15 => ("SIN", 1),
    16 => ("COS", 1),
    17 => ("TAN", 1),
    18 => ("ATAN", 1),
    19 => ("PI", 0),
    20 => ("SQRT", 1),
    21 => ("EXP", 1),
    22 => ("LN", 1),
    23 => ("LOG10", 1),
    24 => ("ABS", 1),
    25 => ("INT", 1),
    26 => ("SIGN", 1),
    32 => ("LEN", 1),
    33 => ("VALUE", 1),
    34 => ("TRUE", 0),
    35 => ("FALSE", 0),
    38 => ("NOT", 1),
    39 => ("MOD", 2),
    _ => return None,
  })
}

const fn formula_function_name(index: u16) -> Option<&'static str> {
  Some(match index {
    0 => "COUNT",
    1 => "IF",
    4 => "SUM",
    5 => "AVERAGE",
    6 => "MIN",
    7 => "MAX",
    11 => "NPV",
    12 => "STDEV",
    13 => "DOLLAR",
    14 => "FIXED",
    27 => "ROUND",
    28 => "LOOKUP",
    29 => "INDEX",
    30 => "REPT",
    31 => "MID",
    36 => "AND",
    37 => "OR",
    46 => "VAR",
    48 => "TEXT",
    56 => "PV",
    57 => "FV",
    58 => "NPER",
    59 => "PMT",
    60 => "RATE",
    61 => "MIRR",
    62 => "IRR",
    63 => "RAND",
    64 => "MATCH",
    65 => "DATE",
    66 => "TIME",
    67 => "DAY",
    68 => "MONTH",
    69 => "YEAR",
    70 => "WEEKDAY",
    71 => "HOUR",
    72 => "MINUTE",
    73 => "SECOND",
    74 => "NOW",
    97 => "ATAN2",
    98 => "ASIN",
    99 => "ACOS",
    100 => "CHOOSE",
    101 => "HLOOKUP",
    102 => "VLOOKUP",
    109 => "LOG",
    111 => "CHAR",
    112 => "LOWER",
    113 => "UPPER",
    114 => "PROPER",
    115 => "LEFT",
    116 => "RIGHT",
    117 => "EXACT",
    118 => "TRIM",
    119 => "REPLACE",
    120 => "SUBSTITUTE",
    121 => "CODE",
    124 => "FIND",
    125 => "CELL",
    126 => "ISERR",
    127 => "ISTEXT",
    128 => "ISNUMBER",
    129 => "ISBLANK",
    130 => "T",
    131 => "N",
    148 => "INDIRECT",
    162 => "CLEAN",
    163 => "MDETERM",
    164 => "MINVERSE",
    165 => "MMULT",
    169 => "COUNTA",
    183 => "PRODUCT",
    184 => "FACT",
    190 => "ISNONTEXT",
    193 => "STDEVP",
    194 => "VARP",
    197 => "TRUNC",
    212 => "ROUNDUP",
    213 => "ROUNDDOWN",
    216 => "RANK",
    220 => "DAYS360",
    221 => "TODAY",
    228 => "SUMPRODUCT",
    269 => "AVEDEV",
    276 => "COMBIN",
    279 => "EVEN",
    298 => "ODD",
    300 => "POISSON",
    303 => "SUMXMY2",
    318 => "DEVSQ",
    319 => "GEOMEAN",
    320 => "HARMEAN",
    321 => "SUMSQ",
    325 => "LARGE",
    326 => "SMALL",
    327 => "QUARTILE",
    328 => "PERCENTILE",
    329 => "PERCENTRANK",
    330 => "MODE",
    331 => "TRIMMEAN",
    336 => "CONCATENATE",
    337 => "POWER",
    342 => "RADIANS",
    343 => "DEGREES",
    344 => "SUBTOTAL",
    345 => "SUMIF",
    346 => "COUNTIF",
    347 => "COUNTBLANK",
    _ => return None,
  })
}

struct XlsCellConversionContext<'a, 'source> {
  view: &'a XlsWorkbookView<'source>,
  index: &'a olecfsdk::xls::XlsSparseCellIndex<'source>,
  sheet_index: usize,
  options: ConversionOptions,
  xf_unmapped: &'a [bool],
  phonetic_visible_ranges: &'a [CellRange],
}

fn convert_cell(
  context: &XlsCellConversionContext<'_, '_>,
  source: olecfsdk::xls::XlsCellRef<'_>,
  report: &mut ConversionReport,
) -> Result<Cell> {
  let header = source.cell();
  let source_location = SourceLocation::XlsCell {
    workbook_index: 0,
    sheet_index: context.sheet_index,
    row: header.row,
    column: header.column,
  };
  if context
    .xf_unmapped
    .get(usize::from(header.format_index))
    .copied()
    .unwrap_or(true)
  {
    unsupported(
      report,
      context.options,
      ConversionCode::CellFormattingNotMapped,
      source_location,
    )?;
  }
  let cell_formula = if matches!(source.value(), XlsCellValueRef::Formula(_)) {
    context
      .index
      .resolve_cell_formula(source)?
      .and_then(|formula| render_cell_formula(formula, header.row, header.column))
  } else {
    None
  };
  if matches!(
    source.value(),
    XlsCellValueRef::Formula(_) | XlsCellValueRef::Formula4Compatibility(_)
  ) && cell_formula.is_none()
  {
    unsupported(
      report,
      context.options,
      ConversionCode::FormulaNotMapped,
      source_location,
    )?;
  }

  let (data_type, value) = if let Some(label) = source.label_sst() {
    (
      Some(CellValues::SharedString),
      Some(label.shared_string_index.to_string()),
    )
  } else {
    let value = context.view.resolve_cell_value(context.index, source)?;
    convert_cell_value(value, source_location, context.options, report)?
  };
  report.record(Disposition::Mapped);
  Ok(Cell {
    cell_reference: Some(cell_reference(header.row, header.column)),
    style_index: Some(u32::from(header.format_index)),
    data_type,
    show_phonetic: bool_attribute(phonetic_is_visible(
      context.phonetic_visible_ranges,
      header.row,
      header.column,
    )),
    cell_formula,
    cell_value: value.map(|value| CellValue(xstring(value))),
    ..Default::default()
  })
}

fn convert_cell_value(
  value: XlsCellValue,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<(Option<CellValues>, Option<String>)> {
  match value {
    XlsCellValue::Blank => Ok((None, None)),
    XlsCellValue::Number(value) => Ok((Some(CellValues::Number), Some(value.to_string()))),
    XlsCellValue::Boolean(value) => Ok((
      Some(CellValues::Boolean),
      Some(if value { "1" } else { "0" }.to_owned()),
    )),
    XlsCellValue::Error(value) => Ok((
      Some(CellValues::Error),
      Some(cell_error_value(value).to_owned()),
    )),
    XlsCellValue::String(value) => Ok((Some(CellValues::String), Some(value))),
    XlsCellValue::Formula(value) => convert_formula_cache(value),
    XlsCellValue::CompatibilityBoolErr { .. } => {
      unsupported(
        report,
        options,
        ConversionCode::CompatibilityCellValue,
        source,
      )?;
      Ok((None, None))
    }
  }
}

fn convert_formula_cache(
  value: XlsFormulaCachedValue,
) -> Result<(Option<CellValues>, Option<String>)> {
  Ok(match value {
    XlsFormulaCachedValue::Number(value) => (Some(CellValues::Number), Some(value.to_string())),
    XlsFormulaCachedValue::String(value) => (Some(CellValues::String), Some(value)),
    XlsFormulaCachedValue::Boolean(value) => (
      Some(CellValues::Boolean),
      Some(if value { "1" } else { "0" }.to_owned()),
    ),
    XlsFormulaCachedValue::Error(value) => (
      Some(CellValues::Error),
      Some(cell_error_value(value).to_owned()),
    ),
    XlsFormulaCachedValue::Empty => (None, None),
  })
}

fn convert_sheet_state(
  state: u8,
  source: SourceLocation,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<SheetStateValues>> {
  if state & !0x03 != 0 || state & 0x03 == 3 {
    unsupported(report, options, ConversionCode::SheetStateNotMapped, source)?;
    return Ok(None);
  }
  Ok(match state & 0x03 {
    0 => None,
    1 => Some(SheetStateValues::Hidden),
    2 => Some(SheetStateValues::VeryHidden),
    _ => unreachable!(),
  })
}

fn cell_reference(row: u16, column: u16) -> String {
  let mut column = u32::from(column) + 1;
  let mut reversed = [0_u8; 4];
  let mut len = 0;
  while column != 0 {
    column -= 1;
    reversed[len] = b'A' + (column % 26) as u8;
    len += 1;
    column /= 26;
  }
  let mut value = String::with_capacity(len + 5);
  value.extend(reversed[..len].iter().rev().map(|value| char::from(*value)));
  value.push_str(&(u32::from(row) + 1).to_string());
  value
}

fn cell_range_reference(
  first_row: u16,
  first_column: u16,
  last_row: u16,
  last_column: u16,
) -> String {
  let first = cell_reference(first_row, first_column);
  let last = cell_reference(last_row, last_column);
  let mut value = String::with_capacity(first.len() + last.len() + 1);
  value.push_str(&first);
  value.push(':');
  value.push_str(&last);
  value
}

fn cell_reference_u32(row: u32, column: u32) -> Option<String> {
  if row > 1_048_575 || column > 16_383 {
    return None;
  }
  let mut column = column + 1;
  let mut reversed = [0_u8; 3];
  let mut len = 0;
  while column != 0 {
    column -= 1;
    reversed[len] = b'A' + (column % 26) as u8;
    len += 1;
    column /= 26;
  }
  let mut value = String::with_capacity(len + 7);
  value.extend(reversed[..len].iter().rev().map(|value| char::from(*value)));
  value.push_str(&(row + 1).to_string());
  Some(value)
}

fn cell_range_reference_u32(
  first_row: u32,
  first_column: u32,
  last_row: u32,
  last_column: u32,
) -> Option<String> {
  if first_row > last_row || first_column > last_column {
    return None;
  }
  let first = cell_reference_u32(first_row, first_column)?;
  let last = cell_reference_u32(last_row, last_column)?;
  Some(format!("{first}:{last}"))
}

fn xstring(value: String) -> XstringType {
  let preserve = value.starts_with(char::is_whitespace)
    || value.ends_with(char::is_whitespace)
    || value.contains("  ");
  XstringType {
    space: preserve.then_some(SpaceProcessingModeValues::Preserve),
    xml_content: Some(value),
  }
}

const fn cell_error_value(value: CellErrorCode) -> &'static str {
  match value {
    CellErrorCode::Null => "#NULL!",
    CellErrorCode::DivisionByZero => "#DIV/0!",
    CellErrorCode::Value => "#VALUE!",
    CellErrorCode::Reference => "#REF!",
    CellErrorCode::Name => "#NAME?",
    CellErrorCode::Number => "#NUM!",
    CellErrorCode::NotAvailable => "#N/A",
    CellErrorCode::GettingData => "#GETTING_DATA",
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
  use olecfsdk::xls::{
    AutoFilter12Continuation, AutoFilter12Criterion, AutoFilter12Flags, CellRange, FrtFlags,
    FrtHeader, FrtRefHeaderU, PrintSetupOptions, SheetExtColorIndex, SheetExtOptional,
    SheetExtOptionalFlags, VerticalPageBreak, VerticalPageBreaksRecord,
  };

  const TEST_LOCATION: SourceLocation = SourceLocation::XlsSheet {
    workbook_index: 0,
    sheet_index: 0,
  };
  const REPORTING: ConversionOptions = ConversionOptions {
    unsupported: LossPolicy::Report,
  };

  #[test]
  fn worksheet_phonetic_visibility_and_guts_levels_follow_ms_xls() {
    let ranges = [
      CellRange {
        first_row: 2,
        last_row: 4,
        first_column: 3,
        last_column: 7,
      },
      CellRange {
        first_row: 10,
        last_row: 10,
        first_column: 1,
        last_column: 1,
      },
    ];
    assert!(phonetic_is_visible(&ranges, 2, 3));
    assert!(phonetic_is_visible(&ranges, 4, 7));
    assert!(phonetic_is_visible(&ranges, 10, 1));
    assert!(!phonetic_is_visible(&ranges, 1, 3));
    assert!(!phonetic_is_visible(&ranges, 4, 8));

    assert_eq!(guts_outline_level(0), None);
    assert_eq!(guts_outline_level(2), Some(1));
    assert_eq!(guts_outline_level(8), Some(7));
    assert!(!guts_outline_level_is_valid(1));
    assert!(!guts_outline_level_is_valid(9));
  }

  #[test]
  fn worksheet_record_audit_only_accepts_explicitly_accounted_records() {
    assert!(worksheet_record_is_accounted_for(&BiffRecordData::Eof));
    assert!(worksheet_record_is_accounted_for(
      &BiffRecordData::PhoneticInfo(olecfsdk::xls::PhoneticInfoRecord {
        font_index: 0,
        flags: olecfsdk::xls::PhoneticFlags::empty(),
        range_count: 0,
        ranges: Vec::new(),
      })
    ));
    assert!(!worksheet_record_is_accounted_for(
      &BiffRecordData::Unknown {
        record_type: 0x7ffe,
        payload: vec![1, 2, 3],
      }
    ));
  }

  #[test]
  fn worksheet_property_direction_and_tab_colors_follow_ms_xls_semantics() {
    assert!(summary_columns_on_right(false, false));
    assert!(!summary_columns_on_right(true, false));
    assert!(!summary_columns_on_right(false, true));
    assert!(summary_columns_on_right(true, true));

    let mut source = SheetExtRecord {
      header: FrtHeader {
        record_type: 0x0862,
        flags: FrtFlags::empty(),
        reserved: 0,
      },
      declared_size: 40,
      tab_color: SheetExtColorIndex {
        color_index: 8,
        reserved: 0,
      },
      optional: Some(SheetExtOptional {
        flags: SheetExtOptionalFlags {
          color_index: 8,
          calculate_conditional_formats: true,
          not_published: false,
          reserved: 0,
        },
        color: CfColor {
          color_type: 2,
          color_value: 0x8040_2010,
          tint_bits: 0.25_f64.to_bits(),
        },
      }),
    };
    let rgb = convert_sheet_tab_color(&source).expect("matching modern tab color maps");
    assert_eq!(rgb.rgb.as_deref(), Some("80102040"));
    assert_eq!(rgb.tint, Some(0.25));
    assert_eq!(rgb.indexed, None);

    let optional = source.optional.as_mut().unwrap();
    optional.flags.color_index = 9;
    optional.color.color_type = 3;
    optional.color.color_value = 6;
    let indexed = convert_sheet_tab_color(&source).expect("older tab color takes precedence");
    assert_eq!(indexed.indexed, Some(8));
    assert_eq!(indexed.theme, None);

    source.tab_color.color_index = 9;
    let themed = convert_sheet_tab_color(&source).expect("matching theme tab color maps");
    assert_eq!(themed.theme, Some(6));
    assert_eq!(themed.tint, Some(0.25));

    source.tab_color.color_index = 0x7f;
    assert_eq!(convert_sheet_tab_color(&source), None);
  }

  #[test]
  fn print_setup_maps_signed_start_and_every_named_option() {
    let source = PrintSetupRecord {
      paper_size: 9,
      scale: 125,
      page_start: -3,
      fit_width: 2,
      fit_height: 0,
      options: PrintSetupOptions {
        left_to_right: true,
        portrait: false,
        no_printer_settings: false,
        black_and_white: true,
        draft: true,
        print_comments: true,
        no_orientation: false,
        use_first_page_number: true,
        unused: false,
        comments_at_end: false,
        print_errors: 3,
        reserved: 0,
      },
      horizontal_resolution: 600,
      vertical_resolution: 300,
      header_margin_bits: 0.3_f64.to_bits(),
      footer_margin_bits: 0.4_f64.to_bits(),
      copies: 2,
    };
    let mut report = ConversionReport::default();
    let target = convert_page_setup(&source, TEST_LOCATION, REPORTING, &mut report).unwrap();
    assert!(report.issues().is_empty());
    assert_eq!(target.paper_size, Some(9));
    assert_eq!(target.scale, Some(125));
    assert_eq!(target.first_page_number, Some(-3));
    assert_eq!(target.fit_to_width, Some(2));
    assert_eq!(target.fit_to_height, Some(0));
    assert_eq!(target.page_order, Some(PageOrderValues::OverThenDown));
    assert_eq!(target.orientation, Some(OrientationValues::Landscape));
    assert_eq!(target.cell_comments, Some(CellCommentsValues::AsDisplayed));
    assert_eq!(target.errors, Some(PrintErrorValues::Na));
    assert_eq!(target.horizontal_dpi, Some(600));
    assert_eq!(target.vertical_dpi, Some(300));
    assert_eq!(target.copies, Some(2));
  }

  #[test]
  fn vertical_page_breaks_map_column_and_row_interval_without_offset() {
    let source = VerticalPageBreaksRecord {
      break_count: 2,
      breaks: vec![
        VerticalPageBreak {
          column: 3,
          first_row: 0,
          last_row: 20,
        },
        VerticalPageBreak {
          column: 7,
          first_row: 4,
          last_row: 40,
        },
      ],
    };
    let mut report = ConversionReport::default();
    let target = convert_column_breaks(&[&source], TEST_LOCATION, REPORTING, &mut report)
      .unwrap()
      .expect("two page breaks map");
    assert!(report.issues().is_empty());
    assert_eq!(target.count, Some(2));
    assert_eq!(target.manual_break_count, Some(2));
    assert_eq!(target.r#break[0].id, Some(3));
    assert_eq!(target.r#break[0].min, Some(0));
    assert_eq!(target.r#break[0].max, Some(20));
    assert_eq!(
      target.r#break[0]
        .manual_page_break
        .map(BooleanValue::as_bool),
      Some(true)
    );
    assert_eq!(target.r#break[1].id, Some(7));
  }

  #[test]
  fn future_auto_filters_map_dynamic_date_and_icon_variants() {
    let range = CellRange {
      first_row: 1,
      last_row: 20,
      first_column: 2,
      last_column: 4,
    };
    let continuation_header = FrtRefHeaderU {
      record_type: 0x087f,
      flags: FrtFlags::HAS_CELL_RANGE,
      range,
    };
    let base = AutoFilter12Record {
      header: FrtRefHeaderU {
        record_type: 0x087e,
        flags: FrtFlags::HAS_CELL_RANGE,
        range,
      },
      entry_index: 1,
      hide_arrow: true,
      dynamic_filter_type: AutoFilter12DynamicFilter::Today,
      declared_criteria_count: 2,
      declared_date_grouping_count: 0,
      flags: AutoFilter12Flags {
        worksheet: true,
        unused: 0,
      },
      unused: 0,
      list_id: u32::MAX,
      user_view_guid: [0; 16],
      filter: AutoFilter12Filter::Criteria,
      criteria: vec![
        AutoFilter12Continuation {
          header: continuation_header,
          value: AutoFilter12Criterion {
            operand: AutoFilterOperand {
              comparison: 6,
              value: AutoFilterOperandValue::Number {
                bits: 10.0_f64.to_bits(),
              },
              string: None,
            },
            string_unused: None,
          },
        },
        AutoFilter12Continuation {
          header: continuation_header,
          value: AutoFilter12Criterion {
            operand: AutoFilterOperand {
              comparison: 1,
              value: AutoFilterOperandValue::Number {
                bits: 20.0_f64.to_bits(),
              },
              string: None,
            },
            string_unused: None,
          },
        },
      ],
      date_groupings: Vec::new(),
    };
    let mut report = ConversionReport::default();
    let dynamic = convert_future_filter_column(&base, TEST_LOCATION, REPORTING, &mut report)
      .expect("dynamic AutoFilter12 maps");
    assert!(report.issues().is_empty());
    assert_eq!(dynamic.column_id, 1);
    assert_eq!(dynamic.hidden_button.map(BooleanValue::as_bool), Some(true));
    assert!(matches!(
      dynamic.filter_column_choice,
      Some(x::FilterColumnChoice::DynamicFilter(x::DynamicFilter {
        r#type: x::DynamicFilterValues::Today,
        val: Some(10.0),
        max_val: Some(20.0),
        ..
      }))
    ));

    let mut date = base.clone();
    date.dynamic_filter_type = AutoFilter12DynamicFilter::None;
    date.declared_criteria_count = 0;
    date.criteria.clear();
    date.declared_date_grouping_count = 1;
    date.date_groupings.push(AutoFilter12Continuation {
      header: continuation_header,
      value: AutoFilter12DateGrouping {
        year: 2026,
        month: 7,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        unused: 0,
        reserved: 0,
        level: AutoFilter12DateGroupingLevel::Month,
      },
    });
    let date = convert_future_filter_column(&date, TEST_LOCATION, REPORTING, &mut report)
      .expect("date AutoFilter12 maps");
    let Some(x::FilterColumnChoice::Filters(filters)) = date.filter_column_choice else {
      panic!("date grouping maps to filters/dateGroupItem")
    };
    let x::FiltersChoice::DateGroupItem(group) = &filters.filters_choice[0] else {
      panic!("date grouping remains typed")
    };
    assert_eq!(group.year, 2026);
    assert_eq!(group.month, Some(7));
    assert_eq!(group.day, None);

    let mut icon = base;
    icon.dynamic_filter_type = AutoFilter12DynamicFilter::None;
    icon.declared_criteria_count = 0;
    icon.criteria.clear();
    icon.filter = AutoFilter12Filter::Icon {
      icon_set: KpiSet::ThreeFlags,
      icon_index: 2,
    };
    let icon = convert_future_filter_column(&icon, TEST_LOCATION, REPORTING, &mut report)
      .expect("icon AutoFilter12 maps");
    assert!(matches!(
      icon.filter_column_choice,
      Some(x::FilterColumnChoice::XIconFilter(x::IconFilter {
        icon_set: x::IconSetValues::ThreeFlags,
        icon_id: Some(2),
      }))
    ));
  }
}
