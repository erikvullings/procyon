use fontique::{
  Attributes as FontAttributes, Collection as FontCollection,
  CollectionOptions as FontCollectionOptions, FontStyle, FontWeight, FontWidth, GenericFamily,
  QueryFamily, QueryStatus, SourceCache,
};
use image::codecs::png::PngEncoder;
use image::{ColorType, ImageEncoder};
use skrifa::outline::{
  DrawSettings, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::prelude::{FontRef, LocationRef, MetadataProvider, Size as FontSize};
use skrifa::raw::types::Tag as FontTableTag;
use std::collections::HashMap;
use thiserror::Error;
use tiny_skia::{
  FillRule as TinySkiaFillRule, Mask as TinySkiaMask, Path as TinySkiaPath,
  PathBuilder as TinySkiaPathBuilder, PathSegment as TinySkiaPathSegment, Point as TinySkiaPoint,
  Transform as TinySkiaTransform,
};

use crate::bitmap::{
  BitmapCompression, DeviceIndependentBitmap, DibColorTable, DibColorUsage, DibHeader,
};
use crate::common::{Reader, SdkEnumValue};
use crate::emf::{
  EmfRecordData, EmfRecordRef, EmrAlphaBlend, EmrAlphaFormat, EmrMapMode, EmrPenLineStyle,
};
use crate::emfplus::{
  EmfPlusBitmapPayload, EmfPlusBrushData, EmfPlusBrushRef, EmfPlusDrawArcData,
  EmfPlusDrawImageData, EmfPlusDrawImagePointsData, EmfPlusDrawPointsData,
  EmfPlusDrawRectShapeData, EmfPlusDrawStringData, EmfPlusFillPieData, EmfPlusFillRectShapeData,
  EmfPlusFontObject, EmfPlusHatchStyle, EmfPlusImageData, EmfPlusImageObject,
  EmfPlusObjectAssembler, EmfPlusObjectData, EmfPlusObjectRecordData, EmfPlusPathObject,
  EmfPlusPathPointType, EmfPlusPathPointTypeFlags, EmfPlusPathPointTypeValue,
  EmfPlusPathPointTypes, EmfPlusPenObject, EmfPlusPointData, EmfPlusRecord, EmfPlusRecordData,
  EmfPlusRecordFlags, EmfPlusRecordType, EmfPlusRegionNodeDataType,
  EmfPlusRotateWorldTransformData, EmfPlusScaleWorldTransformData,
  EmfPlusTranslateWorldTransformData, EmfPlusUnitType,
};
use crate::wmf::{
  WmfBinaryRasterOperation, WmfBrushStyle, WmfEscapeData, WmfExtTextOutOptions, WmfMetafileRef,
  WmfPenLineStyle, WmfRecordData, WmfTernaryRasterOperationCode, WmfTextAlignmentModeFlags,
};

// record ids. The byte offsets below are record-relative, including the
// 8-byte EMR header, as specified by [MS-EMF].
#[cfg(test)]
const EMF_HEADER_SIZE: usize = 108;
const EMF_RECORD_HEADER_SIZE: usize = 8;
const EMF_BOUNDS_LEFT_OFFSET: usize = 8;
const EMF_BOUNDS_TOP_OFFSET: usize = 12;
const EMF_BOUNDS_RIGHT_OFFSET: usize = 16;
const EMF_BOUNDS_BOTTOM_OFFSET: usize = 20;
const EMF_FRAME_LEFT_OFFSET: usize = 24;
const EMF_FRAME_TOP_OFFSET: usize = 28;
const EMF_FRAME_RIGHT_OFFSET: usize = 32;
const EMF_FRAME_BOTTOM_OFFSET: usize = 36;
const EMF_DEVICE_WIDTH_OFFSET: usize = 72;
const EMF_DEVICE_HEIGHT_OFFSET: usize = 76;
const EMF_MILLIMETERS_WIDTH_OFFSET: usize = 80;
const EMF_MILLIMETERS_HEIGHT_OFFSET: usize = 84;
const EMR_EOF: u32 = 14;
const EMR_POLYBEZIER: u32 = 2;
const EMR_POLYGON: u32 = 3;
const EMR_POLYLINE: u32 = 4;
const EMR_POLYBEZIER_TO: u32 = 5;
const EMR_POLYLINE_TO: u32 = 6;
const EMR_POLYPOLYLINE: u32 = 7;
const EMR_POLYPOLYGON: u32 = 8;
const EMR_SET_WINDOW_EXT_EX: u32 = 9;
const EMR_SET_WINDOW_ORG_EX: u32 = 10;
const EMR_SET_VIEWPORT_EXT_EX: u32 = 11;
const EMR_SET_VIEWPORT_ORG_EX: u32 = 12;
const EMR_SET_PIXEL_V: u32 = 15;
const EMR_SET_MAP_MODE: u32 = 17;
const EMR_SET_ROP_2: u32 = 20;
const EMR_SET_TEXT_ALIGN: u32 = 22;
const EMR_SET_TEXT_COLOR: u32 = 24;
const EMR_OFFSET_CLIP_RGN: u32 = 26;
const EMR_MOVE_TO_EX: u32 = 27;
const EMR_SET_META_RGN: u32 = 28;
const EMR_EXCLUDE_CLIP_RECT: u32 = 29;
const EMR_INTERSECT_CLIP_RECT: u32 = 30;
const EMR_SCALE_VIEWPORT_EXT_EX: u32 = 31;
const EMR_SCALE_WINDOW_EXT_EX: u32 = 32;
const EMR_SAVE_DC: u32 = 33;
const EMR_RESTORE_DC: u32 = 34;
const EMR_SET_WORLD_TRANSFORM: u32 = 35;
const EMR_MODIFY_WORLD_TRANSFORM: u32 = 36;
const EMR_SELECT_OBJECT: u32 = 37;
const EMR_CREATE_PEN: u32 = 38;
const EMR_CREATE_BRUSH_INDIRECT: u32 = 39;
const EMR_DELETE_OBJECT: u32 = 40;
const EMR_ELLIPSE: u32 = 42;
const EMR_RECTANGLE: u32 = 43;
const EMR_ROUND_RECT: u32 = 44;
const EMR_ARC: u32 = 45;
const EMR_CHORD: u32 = 46;
const EMR_PIE: u32 = 47;
const EMR_LINE_TO: u32 = 54;
const EMR_BIT_BLT: u32 = 76;
const EMR_STRETCH_BLT: u32 = 77;
const EMR_MASK_BLT: u32 = 78;
const EMR_SET_DIBITS_TO_DEVICE: u32 = 80;
const EMR_STRETCH_DIBITS: u32 = 81;
const EMR_EXT_CREATE_FONT_INDIRECT_W: u32 = 82;
const EMR_EXT_TEXTOUT_A: u32 = 83;
const EMR_EXT_TEXTOUT_W: u32 = 84;
const EMR_POLYBEZIER16: u32 = 85;
const EMR_POLYGON16: u32 = 86;
const EMR_POLYLINE16: u32 = 87;
const EMR_POLYBEZIER_TO16: u32 = 88;
const EMR_POLYLINE_TO16: u32 = 89;
const EMR_POLYPOLYLINE16: u32 = 90;
const EMR_POLYPOLYGON16: u32 = 91;
const EMR_EXT_CREATE_PEN: u32 = 95;
const EMR_ALPHA_BLEND: u32 = 114;
const EMR_SELECT_CLIP_PATH: u32 = 67;
const EMR_EXT_SELECT_CLIP_RGN: u32 = 75;
const EMR_BITMAP_DEST_X_OFFSET: usize = 24;
const EMR_BITMAP_DEST_Y_OFFSET: usize = 28;
const EMR_BITMAP_SOURCE_WIDTH_OFFSET: usize = 40;
const EMR_BITMAP_SOURCE_HEIGHT_OFFSET: usize = 44;
const EMR_BITMAP_INFO_OFFSET_OFFSET: usize = 48;
const EMR_BITMAP_INFO_SIZE_OFFSET: usize = 52;
const EMR_BITMAP_BITS_OFFSET_OFFSET: usize = 56;
const EMR_BITMAP_BITS_SIZE_OFFSET: usize = 60;
const EMR_BITMAP_COLOR_USAGE_OFFSET: usize = 64;
const EMR_STRETCH_DIBITS_ROP_OFFSET: usize = 68;
const EMR_STRETCH_DIBITS_DEST_WIDTH_OFFSET: usize = 72;
const EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET: usize = 76;
const EMR_BLT_DEST_WIDTH_OFFSET: usize = 32;
const EMR_BLT_DEST_HEIGHT_OFFSET: usize = 36;
const EMR_BLT_ROP_OFFSET: usize = 40;
const EMR_BLT_SOURCE_X_OFFSET: usize = 44;
const EMR_BLT_SOURCE_Y_OFFSET: usize = 48;
const EMR_BLT_COLOR_USAGE_OFFSET: usize = 80;
const EMR_BLT_INFO_OFFSET_OFFSET: usize = 84;
const EMR_BLT_INFO_SIZE_OFFSET: usize = 88;
const EMR_BLT_BITS_OFFSET_OFFSET: usize = 92;
const EMR_BLT_BITS_SIZE_OFFSET: usize = 96;
const EMR_STRETCH_BLT_SOURCE_WIDTH_OFFSET: usize = 100;
const EMR_STRETCH_BLT_SOURCE_HEIGHT_OFFSET: usize = 104;
const ENHMETA_STOCK_OBJECT: u32 = 0x8000_0000;
const WHITE_BRUSH: u32 = ENHMETA_STOCK_OBJECT;
const BLACK_BRUSH: u32 = ENHMETA_STOCK_OBJECT | 4;
const NULL_BRUSH: u32 = ENHMETA_STOCK_OBJECT | 5;
const WHITE_PEN: u32 = ENHMETA_STOCK_OBJECT | 6;
const BLACK_PEN: u32 = ENHMETA_STOCK_OBJECT | 7;
const NULL_PEN: u32 = ENHMETA_STOCK_OBJECT | 8;
const MWT_IDENTITY: u32 = 1;
const MWT_LEFTMULTIPLY: u32 = 2;
const MWT_RIGHTMULTIPLY: u32 = 3;
const MWT_SET: u32 = 4;
const EMR_COMMENT: u32 = 70;
const EMR_COMMENT_EMFPLUS: u32 = 0x2B46_4D45;
const LOGFONT_FACE_NAME_CHARS: usize = 32;
// values and keeps DIB scanlines aligned to four bytes.
const RGB_BYTES_PER_PIXEL: usize = 3;
const BGRA_BYTES_PER_PIXEL: usize = 4;
#[cfg(test)]
const BI_RGB: u32 = 0;
#[cfg(test)]
const BI_PNG: u32 = 5;
const DEFAULT_RENDER_WIDTH: usize = 1024;
const DEFAULT_RENDER_HEIGHT: usize = 768;
const DEFAULT_MAX_PIXELS: usize = 16_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedMetafile {
  pub data: Vec<u8>,
  pub content_type: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetafilePhysicalSize {
  pub width_pt: f32,
  pub height_pt: f32,
  pub natural_width_px: u32,
  pub natural_height_px: u32,
}

/// The external physical header supplied when a non-placeable WMF is played.
///
/// `METAFILEPICT.xExt` and `yExt` are expressed in hundredths of a
/// millimetre. Office supplies this header with `MM_ANISOTROPIC` for ActiveX
/// and OLE replacement graphics whose WMF byte stream has no placeable
/// header. Win32 also requires the resolution of the reference device context
/// to realize that physical rectangle as a device grid. The WMF's own
/// `META_SETWINDOWEXT` records continue to establish logical units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WmfExternalHeader {
  pub width_hundredths_mm: u32,
  pub height_hundredths_mm: u32,
  pub reference_device_dpi_x: u32,
  pub reference_device_dpi_y: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderOptions {
  pub target_width_px: Option<u32>,
  pub target_height_px: Option<u32>,
  pub max_pixels: Option<u32>,
  /// Preserve an unpainted destination as transparent output.
  ///
  /// GDI raster operations still require concrete destination samples. The
  /// renderer therefore replays against black and white destinations and
  /// reconstructs straight color plus coverage from the two results. This is
  /// the same black/white-background technique used by metafile consumers
  /// when a host application composites the preview over its own fill.
  pub transparent_background: bool,
  /// Existing destination surface color used when replaying raster operations.
  ///
  /// Metafiles embedded in a filled host shape paint onto that shape instead
  /// of an implicitly white page. Callers that do not supply a destination
  /// retain the standalone white-canvas behavior.
  pub background_color: Option<[u8; 3]>,
  /// Caller-specific realization palette for one-bit DIB pattern brushes.
  ///
  /// This is intentionally opt-in: color-output GDI playback otherwise
  /// preserves the DIB's embedded color table.
  pub monochrome_dib_palette_override: Option<[[u8; 3]; 2]>,
  /// Box-filter one-pixel checkerboard pattern brushes before a fixed output
  /// rescales the raster. This matches filtered GDI+/Cairo image playback and
  /// prevents phase-biased moire in PDF consumers.
  pub filter_high_frequency_pattern_brushes: bool,
  /// Replay text state without painting glyphs into the raster destination.
  ///
  /// This is useful when a fixed-output host lifts metafile text into its own
  /// native text layer. Text alignment and `TA_UPDATECP` state still advance,
  /// and `ETO_OPAQUE` background rectangles remain part of the raster replay.
  pub suppress_text: bool,
  /// Skip solid-brush `PATCOPY` rectangles that a caller lifts into a vector
  /// layer with [`extract_metafile_solid_rects`].
  ///
  /// Other pattern fills and raster operations stay in the replay because
  /// they cannot be separated from the destination surface in general.
  pub suppress_solid_pattern_rects: bool,
  /// Skip standalone `SRCCOPY` DIBs and adjacent masked-bitmap ROP pairs
  /// lifted with [`extract_metafile_bitmap_layers`].
  ///
  /// Only source-copy records and the source-backed transparent-bitmap
  /// combinations recognized by LibreOffice's WMF reader are separable.
  /// Unmatched, non-binary, and destination-dependent bitmap records remain
  /// in the raster replay.
  pub suppress_bitmap_layers: bool,
  /// Physical playback header for a non-placeable WMF.
  ///
  /// This is intentionally caller-supplied rather than inferred from the
  /// byte stream: a standard WMF does not contain `METAFILEPICT.xExt/yExt`.
  /// Placeable WMFs retain their authored header and EMFs retain `Frame`.
  pub wmf_external_header: Option<WmfExternalHeader>,
}

impl RenderOptions {
  fn resolved_canvas_size(self, natural_width: usize, natural_height: usize) -> (usize, usize) {
    let natural_width = natural_width.max(1);
    let natural_height = natural_height.max(1);
    // [MS-WMF] 3.1.3 assigns the window to the metafile and the viewport to
    // the player. A requested output size is therefore the playback viewport,
    // even when it is larger than the metafile's logical extent.
    let resolve_axis =
      |target: Option<u32>, natural: usize| target.map_or(natural, |value| value.max(1) as usize);
    let width = resolve_axis(self.target_width_px, natural_width);
    let height = resolve_axis(self.target_height_px, natural_height);
    clamp_canvas_size(width, height, self.max_pixels)
  }
}

#[derive(Debug, Error)]
pub enum RenderError {
  #[error("{0}")]
  Invalid(String),
}

impl From<String> for RenderError {
  fn from(value: String) -> Self {
    Self::Invalid(value)
  }
}

pub type RenderResult<T> = std::result::Result<T, RenderError>;

mod vector;

pub use vector::{
  MetafileVectorFill, MetafileVectorFillRule, MetafileVectorPoint, MetafileVectorScene,
  extract_metafile_vector_scene, extract_metafile_vector_scene_with_options,
};

pub fn decode_metafile_as_raster(
  data: &[u8],
  content_type: Option<&str>,
) -> RenderResult<Option<DecodedMetafile>> {
  decode_metafile_as_raster_with_options(data, content_type, RenderOptions::default())
}

pub fn decode_metafile_as_raster_with_options(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> RenderResult<Option<DecodedMetafile>> {
  if !looks_like_metafile(data, content_type) {
    return Ok(None);
  }

  if options.transparent_background {
    return decode_transparent_metafile_as_raster(data, content_type, options).map_err(Into::into);
  }

  decode_opaque_metafile_as_raster(data, content_type, options, false, GdiTextSurface::Color)
    .map_err(Into::into)
}

/// Returns the physical playback frame recorded by an EMF header.
///
/// `[MS-EMF]` defines `Frame` in 0.01 millimeter units. The natural pixel
/// dimensions are recovered from the same frame plus the reference
/// `Device`/`Millimeters` fields, matching raster playback.
pub fn metafile_physical_size(
  data: &[u8],
  content_type: Option<&str>,
) -> Option<MetafilePhysicalSize> {
  if !looks_like_metafile(data, content_type) || !is_emf(data) {
    return None;
  }
  emf_physical_size(data)
}

fn decode_opaque_metafile_as_raster(
  data: &[u8],
  _content_type: Option<&str>,
  options: RenderOptions,
  force_vector_replay: bool,
  text_surface: GdiTextSurface,
) -> Result<Option<DecodedMetafile>, String> {
  if let Some(raster) = decode_emf_as_raster(data, options, force_vector_replay, text_surface)? {
    return Ok(Some(raster));
  }

  if let Some(raster) = decode_wmf_as_raster(data, options, text_surface)? {
    return Ok(Some(raster));
  }

  Ok(None)
}

fn decode_transparent_metafile_as_raster(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> Result<Option<DecodedMetafile>, String> {
  let uses_binary_coverage_surface = emf_uses_binary_coverage_surface(data)?;
  let mut black_options = options;
  black_options.transparent_background = false;
  black_options.background_color = Some([0; 3]);
  let mut white_options = options;
  white_options.transparent_background = false;
  white_options.background_color = Some([255; 3]);

  let Some(color_black) = decode_opaque_metafile_as_raster(
    data,
    content_type,
    black_options,
    true,
    GdiTextSurface::Color,
  )?
  else {
    return Ok(None);
  };
  let color_white = decode_opaque_metafile_as_raster(
    data,
    content_type,
    white_options,
    true,
    GdiTextSurface::Color,
  )?
  .ok_or_else(|| "metafile white-background replay produced no raster".to_string())?;
  let mask_black = decode_opaque_metafile_as_raster(
    data,
    content_type,
    black_options,
    true,
    GdiTextSurface::Monochrome,
  )?
  .ok_or_else(|| "metafile monochrome black-background replay produced no raster".to_string())?;
  let mask_white = decode_opaque_metafile_as_raster(
    data,
    content_type,
    white_options,
    true,
    GdiTextSurface::Monochrome,
  )?
  .ok_or_else(|| "metafile monochrome white-background replay produced no raster".to_string())?;
  let color_black = decoded_png_to_rgb(&color_black)?;
  let color_white = decoded_png_to_rgb(&color_white)?;
  let mask_black = decoded_png_to_rgb(&mask_black)?;
  let mask_white = decoded_png_to_rgb(&mask_white)?;
  if color_black.width != color_white.width
    || color_black.height != color_white.height
    || color_black.width != mask_black.width
    || color_black.height != mask_black.height
    || color_black.width != mask_white.width
    || color_black.height != mask_white.height
  {
    return Err("metafile black/white replays have different dimensions".to_string());
  }

  let rgba = if uses_binary_coverage_surface {
    straight_rgba_with_binary_coverage(
      &color_black.rgb,
      &color_white.rgb,
      &mask_black.rgb,
      &mask_white.rgb,
    )?
  } else {
    straight_rgba_from_black_white_with_mask(
      &color_black.rgb,
      &color_white.rgb,
      &mask_black.rgb,
      &mask_white.rgb,
    )?
  };
  Ok(Some(DecodedMetafile {
    data: rgba_to_png(&rgba, color_black.width as u32, color_black.height as u32)?,
    content_type: "image/png",
  }))
}

#[derive(Clone, Debug)]
pub struct MetafileTextRun {
  pub text: String,
  pub x: f32,
  pub y: f32,
  pub font_size: Option<f32>,
  pub font_family: Option<String>,
  pub bold: bool,
  pub italic: bool,
  pub width: Option<f32>,
  /// Normalized distances between consecutive character-cell origins.
  ///
  /// GDI `ExtTextOut` owns glyph placement through its `Dx` array. Keeping
  /// only the summed width loses authored gaps such as MathType's layered
  /// replacement text, where every run starts at the same reference point
  /// and `Dx` carries all horizontal geometry.
  pub advances: Option<Vec<f32>>,
  /// A destination-dependent ternary raster operation occurred earlier in
  /// this record stream.
  ///
  /// Such an operation reads the accumulated playback surface, so a fixed
  /// output backend cannot lift this run into an independent text layer
  /// without changing the preceding bitmap composition. Raw semantic clients
  /// may still consume the run; vector/PDF overlays should keep it in the
  /// raster fallback.
  pub requires_raster_backdrop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetafileSolidRect {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub color: [u8; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetafileBitmapLayer {
  /// A self-contained image for the lifted bitmap layer.
  pub data: Vec<u8>,
  pub content_type: &'static str,
  /// Destination rectangle normalized to the metafile playback viewport.
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub height: f32,
  pub flip_horizontal: bool,
  pub flip_vertical: bool,
}

pub fn extract_metafile_text_runs(data: &[u8], content_type: Option<&str>) -> Vec<MetafileTextRun> {
  extract_metafile_text_runs_with_options(data, content_type, RenderOptions::default())
}

pub fn extract_metafile_text_runs_with_options(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> Vec<MetafileTextRun> {
  if !looks_like_metafile(data, content_type) {
    return Vec::new();
  }
  if is_emf(data) {
    return extract_emf_text_runs(data);
  }
  if crate::wmf::looks_like_wmf(data) {
    return extract_wmf_text_runs(data, options.wmf_external_header);
  }
  Vec::new()
}

/// Returns separable, normalized solid-brush `PATCOPY` rectangles.
///
/// The coordinates use the same normalized playback viewport as
/// [`extract_metafile_text_runs`]. Unsupported brushes and destination-
/// dependent operations are deliberately left in the raster replay.
pub fn extract_metafile_solid_rects(
  data: &[u8],
  content_type: Option<&str>,
) -> Vec<MetafileSolidRect> {
  extract_metafile_solid_rects_with_options(data, content_type, RenderOptions::default())
}

pub fn extract_metafile_solid_rects_with_options(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> Vec<MetafileSolidRect> {
  if !looks_like_metafile(data, content_type) {
    return Vec::new();
  }
  if is_emf(data) {
    return extract_emf_solid_rects(data);
  }
  if crate::wmf::looks_like_wmf(data) {
    return extract_wmf_solid_rects(data, options.wmf_external_header);
  }
  Vec::new()
}

/// Returns separable bitmap layers from WMF DIB records.
///
/// Standalone `SRCCOPY` records are independent opaque layers. LibreOffice's
/// WMF reader additionally recognizes `SRCPAINT` + `SRCAND`, `SRCAND` +
/// `SRCPAINT`, and `SRCAND` + `SRCINVERT` records with the same destination as
/// one transparent bitmap. This extractor requires a binary monochrome mask
/// and matching source geometry for those pairs so unrelated destination-
/// dependent ROPs cannot be lifted accidentally.
pub fn extract_metafile_bitmap_layers(
  data: &[u8],
  content_type: Option<&str>,
) -> Vec<MetafileBitmapLayer> {
  extract_metafile_bitmap_layers_with_options(data, content_type, RenderOptions::default())
}

pub fn extract_metafile_bitmap_layers_with_options(
  data: &[u8],
  content_type: Option<&str>,
  options: RenderOptions,
) -> Vec<MetafileBitmapLayer> {
  if !looks_like_metafile(data, content_type) || !crate::wmf::looks_like_wmf(data) {
    return Vec::new();
  }
  extract_wmf_bitmap_layers(data, options.wmf_external_header)
}

fn extract_emf_text_runs(data: &[u8]) -> Vec<MetafileTextRun> {
  let Some(mut pos) = emf_header_record_size(data) else {
    return Vec::new();
  };
  let mut state = match EmfTextState::new(data) {
    Ok(state) => state,
    Err(_) => return Vec::new(),
  };
  let mut runs = Vec::new();
  let mut requires_raster_backdrop = false;
  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let Ok(record_type) = read_u32(data, pos) else {
      break;
    };
    let Ok(record_size) = read_u32(data, pos + 4) else {
      break;
    };
    let record_size = record_size as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      break;
    }

    requires_raster_backdrop |=
      emf_record_uses_destination_raster(data, pos, record_type, record_size);

    match record_type {
      EMR_SET_WINDOW_ORG_EX if record_size >= 16 => {
        state.window_org_x = read_i32(data, pos + 8).unwrap_or(state.window_org_x);
        state.window_org_y = read_i32(data, pos + 12).unwrap_or(state.window_org_y);
      }
      EMR_SET_WINDOW_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.window_ext_x =
            nonzero_mapping_extent(read_i32(data, pos + 8).unwrap_or(state.window_ext_x));
          state.window_ext_y =
            nonzero_mapping_extent(read_i32(data, pos + 12).unwrap_or(state.window_ext_y));
        }
      }
      EMR_SET_VIEWPORT_ORG_EX if record_size >= 16 => {
        state.viewport_org_x = read_i32(data, pos + 8).unwrap_or(state.viewport_org_x);
        state.viewport_org_y = read_i32(data, pos + 12).unwrap_or(state.viewport_org_y);
      }
      EMR_SET_VIEWPORT_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.viewport_ext_x = read_i32(data, pos + 8).unwrap_or(state.viewport_ext_x);
          state.viewport_ext_y = read_i32(data, pos + 12).unwrap_or(state.viewport_ext_y);
        }
      }
      EMR_SET_MAP_MODE if record_size >= 12 => {
        if let Some(map_mode) = EmrMapMode::from_raw(read_u32(data, pos + 8).unwrap_or_default()) {
          state.map_mode = map_mode;
        }
      }
      EMR_SET_TEXT_ALIGN if record_size >= 12 => {
        state.text_alignment = WmfTextAlignmentModeFlags::from_bits_retain(
          read_u32(data, pos + 8).unwrap_or_default() as u16,
        );
      }
      EMR_MOVE_TO_EX if record_size >= 16 => {
        state.current_pos = EmfPoint {
          x: read_i32(data, pos + 8).unwrap_or(state.current_pos.x),
          y: read_i32(data, pos + 12).unwrap_or(state.current_pos.y),
        };
      }
      EMR_SAVE_DC => state.save(),
      EMR_RESTORE_DC => state.restore(),
      EMR_SET_WORLD_TRANSFORM if record_size >= 32 => {
        if let Ok(transform) = read_xform(data, pos + 8) {
          state.world_transform = transform;
        }
      }
      EMR_MODIFY_WORLD_TRANSFORM if record_size >= 36 => {
        if let (Ok(transform), Ok(mode)) = (read_xform(data, pos + 8), read_u32(data, pos + 32)) {
          state.world_transform = match mode {
            MWT_IDENTITY => EmfTransform::identity(),
            MWT_LEFTMULTIPLY => transform.multiply(state.world_transform),
            MWT_RIGHTMULTIPLY => state.world_transform.multiply(transform),
            MWT_SET => transform,
            _ => state.world_transform,
          };
        }
      }
      EMR_EXT_CREATE_FONT_INDIRECT_W if record_size >= 104 => {
        if let Some((object_id, font)) = read_logfont_object(data, pos, record_size)
          && object_id & ENHMETA_STOCK_OBJECT == 0
        {
          state.fonts.insert(object_id, font);
        }
      }
      EMR_SELECT_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8).unwrap_or(0);
        if state.fonts.contains_key(&object_id) {
          state.current_font = Some(object_id);
        }
      }
      EMR_DELETE_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8).unwrap_or(0);
        state.fonts.remove(&object_id);
        if state.current_font == Some(object_id) {
          state.current_font = None;
        }
      }
      EMR_EXT_TEXTOUT_W => {
        if let Some(text) = extract_semantic_emr_ext_text_out_w(data, pos, record_size)
          && !text.trim().is_empty()
          && let Some(run) = state.text_run(data, pos, record_size, text)
        {
          let mut run = run;
          run.requires_raster_backdrop = requires_raster_backdrop;
          runs.push(run);
        }
      }
      EMR_EXT_TEXTOUT_A => {
        if let Some(text) = extract_emr_ext_text_out_a(data, pos, record_size)
          && !text.trim().is_empty()
          && let Some(run) = state.text_run(data, pos, record_size, text)
        {
          let mut run = run;
          run.requires_raster_backdrop = requires_raster_backdrop;
          runs.push(run);
        }
      }
      EMR_EOF => break,
      _ => {}
    }

    pos += record_size;
  }

  runs
}

fn emf_record_uses_destination_raster(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
) -> bool {
  let operation_uses_destination = |offset| {
    record_size >= offset + 4
      && read_u32(data, record_offset + offset)
        .ok()
        .is_some_and(|raw| emf_ternary_raster_operation(raw).uses_destination())
  };
  match record_type {
    EMR_BIT_BLT | EMR_STRETCH_BLT => operation_uses_destination(EMR_BLT_ROP_OFFSET),
    EMR_STRETCH_DIBITS => operation_uses_destination(EMR_STRETCH_DIBITS_ROP_OFFSET),
    EMR_MASK_BLT if record_size >= EMR_BLT_ROP_OFFSET + 4 => {
      let Ok(rop4) = read_u32(data, record_offset + EMR_BLT_ROP_OFFSET) else {
        return false;
      };
      let background = WmfTernaryRasterOperationCode::from_raw(((rop4 >> 16) & 0xFF) as u8);
      let foreground = WmfTernaryRasterOperationCode::from_raw(((rop4 >> 24) & 0xFF) as u8);
      background.uses_destination() || foreground.uses_destination()
    }
    _ => false,
  }
}

#[derive(Clone, Copy, Debug)]
enum EmfSolidRectClip {
  Infinite,
  Rect((f32, f32, f32, f32)),
  Unsupported,
}

#[derive(Clone, Copy, Debug)]
struct EmfSolidRectSnapshot {
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  current_solid_brush: Option<EmfColor>,
  clip: EmfSolidRectClip,
}

struct EmfSolidRectState {
  width: f32,
  height: f32,
  playback_origin_x: f32,
  playback_origin_y: f32,
  playback_scale_x: f32,
  playback_scale_y: f32,
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  brushes: HashMap<u32, Option<EmfColor>>,
  current_solid_brush: Option<EmfColor>,
  clip: EmfSolidRectClip,
  saved: Vec<EmfSolidRectSnapshot>,
}

impl EmfSolidRectState {
  fn new(data: &[u8]) -> Result<Self, String> {
    let geometry = emf_playback_geometry(data)?;
    Ok(Self {
      width: geometry.width.max(1) as f32,
      height: geometry.height.max(1) as f32,
      playback_origin_x: geometry.origin_x,
      playback_origin_y: geometry.origin_y,
      playback_scale_x: geometry.scale_x,
      playback_scale_y: geometry.scale_y,
      map_mode: EmrMapMode::Text,
      window_org_x: 0,
      window_org_y: 0,
      window_ext_x: geometry.width as i32,
      window_ext_y: geometry.height as i32,
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: geometry.width as i32,
      viewport_ext_y: geometry.height as i32,
      world_transform: EmfTransform::identity(),
      brushes: HashMap::new(),
      current_solid_brush: None,
      clip: EmfSolidRectClip::Infinite,
      saved: Vec::new(),
    })
  }

  fn save(&mut self) {
    self.saved.push(EmfSolidRectSnapshot {
      map_mode: self.map_mode,
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      world_transform: self.world_transform,
      current_solid_brush: self.current_solid_brush,
      clip: self.clip,
    });
  }

  fn restore(&mut self) {
    let Some(saved) = self.saved.pop() else {
      return;
    };
    self.map_mode = saved.map_mode;
    self.window_org_x = saved.window_org_x;
    self.window_org_y = saved.window_org_y;
    self.window_ext_x = saved.window_ext_x;
    self.window_ext_y = saved.window_ext_y;
    self.viewport_org_x = saved.viewport_org_x;
    self.viewport_org_y = saved.viewport_org_y;
    self.viewport_ext_x = saved.viewport_ext_x;
    self.viewport_ext_y = saved.viewport_ext_y;
    self.world_transform = saved.world_transform;
    self.current_solid_brush = saved.current_solid_brush;
    self.clip = saved.clip;
  }

  fn select_object(&mut self, object_id: u32) {
    match object_id {
      WHITE_BRUSH => {
        self.current_solid_brush = Some(EmfColor {
          r: 255,
          g: 255,
          b: 255,
        });
      }
      BLACK_BRUSH => self.current_solid_brush = Some(EmfColor { r: 0, g: 0, b: 0 }),
      NULL_BRUSH => self.current_solid_brush = None,
      // The three gray stock brushes are solid but device-dependent. Keep
      // them in raster replay instead of inventing a portable RGB value.
      value if matches!(value, 0x8000_0001..=0x8000_0003) => {
        self.current_solid_brush = None;
      }
      _ => {
        if let Some(color) = self.brushes.get(&object_id).copied() {
          self.current_solid_brush = color;
        }
      }
    }
  }

  fn map_point(&self, point: EmfPoint) -> (f32, f32) {
    let (x, y) = self.world_transform.apply(point);
    let (scale_x, scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    (
      (self.viewport_org_x as f32 + (x - self.window_org_x as f32) * scale_x
        - self.playback_origin_x)
        * self.playback_scale_x,
      (self.viewport_org_y as f32 + (y - self.window_org_y as f32) * scale_y
        - self.playback_origin_y)
        * self.playback_scale_y,
    )
  }

  fn mapped_axis_aligned_rect(
    &self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
  ) -> Option<(f32, f32, f32, f32)> {
    let first = self.map_point(EmfPoint { x: left, y: top });
    let x_edge = self.map_point(EmfPoint { x: right, y: top });
    let y_edge = self.map_point(EmfPoint { x: left, y: bottom });
    let opposite = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    if ![
      first.0, first.1, x_edge.0, x_edge.1, y_edge.0, y_edge.1, opposite.0, opposite.1,
    ]
    .into_iter()
    .all(f32::is_finite)
    {
      return None;
    }

    let epsilon = 0.01;
    let x_delta = (x_edge.0 - first.0, x_edge.1 - first.1);
    let y_delta = (y_edge.0 - first.0, y_edge.1 - first.1);
    let axis_aligned = (x_delta.1.abs() <= epsilon && y_delta.0.abs() <= epsilon)
      || (x_delta.0.abs() <= epsilon && y_delta.1.abs() <= epsilon);
    let closes = (opposite.0 - (first.0 + x_delta.0 + y_delta.0)).abs() <= epsilon
      && (opposite.1 - (first.1 + x_delta.1 + y_delta.1)).abs() <= epsilon;
    if !axis_aligned || !closes {
      return None;
    }

    Some((
      first.0.min(x_edge.0).min(y_edge.0).min(opposite.0),
      first.1.min(x_edge.1).min(y_edge.1).min(opposite.1),
      first.0.max(x_edge.0).max(y_edge.0).max(opposite.0),
      first.1.max(x_edge.1).max(y_edge.1).max(opposite.1),
    ))
  }

  fn intersect_clip_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let Some(rect) = self.mapped_axis_aligned_rect(left, top, right, bottom) else {
      self.clip = EmfSolidRectClip::Unsupported;
      return;
    };
    self.clip = match self.clip {
      EmfSolidRectClip::Infinite => EmfSolidRectClip::Rect(rect),
      EmfSolidRectClip::Rect(current) => EmfSolidRectClip::Rect(intersect_f32_rects(current, rect)),
      EmfSolidRectClip::Unsupported => EmfSolidRectClip::Unsupported,
    };
  }

  fn solid_rect(&self, x: i32, y: i32, width: i32, height: i32) -> Option<MetafileSolidRect> {
    let color = self.current_solid_brush?;
    let right = x.saturating_add(width);
    let bottom = y.saturating_add(height);
    let mut rect = self.mapped_axis_aligned_rect(x, y, right, bottom)?;
    rect = match self.clip {
      EmfSolidRectClip::Infinite => rect,
      EmfSolidRectClip::Rect(clip) => intersect_f32_rects(rect, clip),
      EmfSolidRectClip::Unsupported => return None,
    };
    let width = rect.2 - rect.0;
    let height = rect.3 - rect.1;
    if width <= 0.0 || height <= 0.0 {
      return None;
    }
    Some(MetafileSolidRect {
      x: rect.0 / self.width,
      y: rect.1 / self.height,
      width: width / self.width,
      height: height / self.height,
      color: [color.r, color.g, color.b],
    })
  }
}

fn intersect_f32_rects(
  first: (f32, f32, f32, f32),
  second: (f32, f32, f32, f32),
) -> (f32, f32, f32, f32) {
  let left = first.0.max(second.0);
  let top = first.1.max(second.1);
  let right = first.2.min(second.2).max(left);
  let bottom = first.3.min(second.3).max(top);
  (left, top, right, bottom)
}

fn scale_emf_extent(extent: i32, numerator: i32, denominator: i32) -> Option<i32> {
  if denominator == 0 {
    return None;
  }
  let scaled = i64::from(extent)
    .checked_mul(i64::from(numerator))?
    .checked_div(i64::from(denominator))?;
  i32::try_from(scaled).ok().map(nonzero_mapping_extent)
}

fn extract_emf_solid_rects(data: &[u8]) -> Vec<MetafileSolidRect> {
  let Some(mut pos) = emf_header_record_size(data) else {
    return Vec::new();
  };
  let Ok(mut state) = EmfSolidRectState::new(data) else {
    return Vec::new();
  };
  let mut rects = Vec::new();
  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let Ok(record_type) = read_u32(data, pos) else {
      break;
    };
    let Ok(record_size) = read_u32(data, pos + 4) else {
      break;
    };
    let record_size = record_size as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      break;
    }

    match record_type {
      EMR_SET_WINDOW_ORG_EX if record_size >= 16 => {
        state.window_org_x = read_i32(data, pos + 8).unwrap_or(state.window_org_x);
        state.window_org_y = read_i32(data, pos + 12).unwrap_or(state.window_org_y);
      }
      EMR_SET_WINDOW_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.window_ext_x =
            nonzero_mapping_extent(read_i32(data, pos + 8).unwrap_or(state.window_ext_x));
          state.window_ext_y =
            nonzero_mapping_extent(read_i32(data, pos + 12).unwrap_or(state.window_ext_y));
        }
      }
      EMR_SET_VIEWPORT_ORG_EX if record_size >= 16 => {
        state.viewport_org_x = read_i32(data, pos + 8).unwrap_or(state.viewport_org_x);
        state.viewport_org_y = read_i32(data, pos + 12).unwrap_or(state.viewport_org_y);
      }
      EMR_SET_VIEWPORT_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.viewport_ext_x =
            nonzero_mapping_extent(read_i32(data, pos + 8).unwrap_or(state.viewport_ext_x));
          state.viewport_ext_y =
            nonzero_mapping_extent(read_i32(data, pos + 12).unwrap_or(state.viewport_ext_y));
        }
      }
      EMR_SCALE_VIEWPORT_EXT_EX if record_size >= 24 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          let scaled_x = scale_emf_extent(
            state.viewport_ext_x,
            read_i32(data, pos + 8).unwrap_or(1),
            read_i32(data, pos + 12).unwrap_or(1),
          );
          let scaled_y = scale_emf_extent(
            state.viewport_ext_y,
            read_i32(data, pos + 16).unwrap_or(1),
            read_i32(data, pos + 20).unwrap_or(1),
          );
          let (Some(x), Some(y)) = (scaled_x, scaled_y) else {
            return Vec::new();
          };
          state.viewport_ext_x = x;
          state.viewport_ext_y = y;
        }
      }
      EMR_SCALE_WINDOW_EXT_EX if record_size >= 24 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          let scaled_x = scale_emf_extent(
            state.window_ext_x,
            read_i32(data, pos + 8).unwrap_or(1),
            read_i32(data, pos + 12).unwrap_or(1),
          );
          let scaled_y = scale_emf_extent(
            state.window_ext_y,
            read_i32(data, pos + 16).unwrap_or(1),
            read_i32(data, pos + 20).unwrap_or(1),
          );
          let (Some(x), Some(y)) = (scaled_x, scaled_y) else {
            return Vec::new();
          };
          state.window_ext_x = x;
          state.window_ext_y = y;
        }
      }
      EMR_SET_MAP_MODE if record_size >= 12 => {
        if let Some(map_mode) = EmrMapMode::from_raw(read_u32(data, pos + 8).unwrap_or_default()) {
          state.map_mode = map_mode;
        }
      }
      EMR_SAVE_DC => state.save(),
      EMR_RESTORE_DC => state.restore(),
      EMR_SET_WORLD_TRANSFORM if record_size >= 32 => {
        if let Ok(transform) = read_xform(data, pos + 8) {
          state.world_transform = transform;
        }
      }
      EMR_MODIFY_WORLD_TRANSFORM if record_size >= 36 => {
        if let (Ok(transform), Ok(mode)) = (read_xform(data, pos + 8), read_u32(data, pos + 32)) {
          state.world_transform = match mode {
            MWT_IDENTITY => EmfTransform::identity(),
            MWT_LEFTMULTIPLY => transform.multiply(state.world_transform),
            MWT_RIGHTMULTIPLY => state.world_transform.multiply(transform),
            MWT_SET => transform,
            _ => state.world_transform,
          };
        }
      }
      EMR_INTERSECT_CLIP_RECT if record_size >= 24 => state.intersect_clip_rect(
        read_i32(data, pos + 8).unwrap_or_default(),
        read_i32(data, pos + 12).unwrap_or_default(),
        read_i32(data, pos + 16).unwrap_or_default(),
        read_i32(data, pos + 20).unwrap_or_default(),
      ),
      EMR_OFFSET_CLIP_RGN
      | EMR_SET_META_RGN
      | EMR_EXCLUDE_CLIP_RECT
      | EMR_SELECT_CLIP_PATH
      | EMR_EXT_SELECT_CLIP_RGN => {
        state.clip = EmfSolidRectClip::Unsupported;
      }
      EMR_CREATE_BRUSH_INDIRECT if record_size >= 24 => {
        let object_id = read_u32(data, pos + 8).unwrap_or(ENHMETA_STOCK_OBJECT);
        if object_id & ENHMETA_STOCK_OBJECT == 0 {
          let brush_style = read_u32(data, pos + 12)
            .ok()
            .and_then(|value| u16::try_from(value).ok())
            .and_then(WmfBrushStyle::from_raw);
          let color = (brush_style == Some(WmfBrushStyle::Solid))
            .then(|| read_color_ref(data, pos + 16).ok())
            .flatten();
          state.brushes.insert(object_id, color);
        }
      }
      EMR_SELECT_OBJECT if record_size >= 12 => {
        state.select_object(read_u32(data, pos + 8).unwrap_or_default());
      }
      EMR_DELETE_OBJECT if record_size >= 12 => {
        state
          .brushes
          .remove(&read_u32(data, pos + 8).unwrap_or_default());
      }
      EMR_BIT_BLT if record_size >= 100 => {
        let rop = read_u32(data, pos + EMR_BLT_ROP_OFFSET)
          .ok()
          .map(emf_ternary_raster_operation);
        let no_source = read_u32(data, pos + EMR_BLT_INFO_SIZE_OFFSET).ok() == Some(0)
          && read_u32(data, pos + EMR_BLT_BITS_SIZE_OFFSET).ok() == Some(0);
        if rop == Some(WmfTernaryRasterOperationCode::PATCOPY)
          && no_source
          && let Some(rect) = state.solid_rect(
            read_i32(data, pos + EMR_BITMAP_DEST_X_OFFSET).unwrap_or_default(),
            read_i32(data, pos + EMR_BITMAP_DEST_Y_OFFSET).unwrap_or_default(),
            read_i32(data, pos + EMR_BLT_DEST_WIDTH_OFFSET).unwrap_or_default(),
            read_i32(data, pos + EMR_BLT_DEST_HEIGHT_OFFSET).unwrap_or_default(),
          )
        {
          rects.push(rect);
        }
      }
      EMR_EOF => break,
      _ => {}
    }

    pos += record_size;
  }
  rects
}

#[derive(Clone)]
struct WmfTextSnapshot {
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  current_pos_x: i32,
  current_pos_y: i32,
  current_font_height: i32,
  current_font_family: Option<String>,
  current_font_char_set: u8,
  current_font_weight: u16,
  current_font_bold: bool,
  current_font_italic: bool,
  current_font_quality: u8,
  text_alignment: WmfTextAlignmentModeFlags,
}

#[derive(Clone, Debug)]
struct WmfTextFont {
  height: i32,
  family: Option<String>,
  char_set: u8,
  weight: u16,
  italic: bool,
  quality: u8,
}

struct WmfTextState {
  natural_width: f32,
  natural_height: f32,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  current_pos_x: i32,
  current_pos_y: i32,
  objects: Vec<Option<WmfTextFont>>,
  current_font_height: i32,
  current_font_family: Option<String>,
  current_font_char_set: u8,
  current_font_weight: u16,
  current_font_bold: bool,
  current_font_italic: bool,
  current_font_quality: u8,
  text_alignment: WmfTextAlignmentModeFlags,
  saved: Vec<WmfTextSnapshot>,
  font_cache: RenderFontCache,
  round_device_coordinates: bool,
}

impl WmfTextState {
  fn new(metafile: &WmfMetafileRef<'_>, external_header: Option<WmfExternalHeader>) -> Self {
    let (window_org_x, window_org_y, window_ext_x, window_ext_y) =
      wmf_initial_window(metafile, external_header);
    Self {
      natural_width: window_ext_x.unsigned_abs().max(1) as f32,
      natural_height: window_ext_y.unsigned_abs().max(1) as f32,
      window_org_x,
      window_org_y,
      window_ext_x: nonzero_mapping_extent(window_ext_x),
      window_ext_y: nonzero_mapping_extent(window_ext_y),
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: mapping_extent_magnitude(window_ext_x),
      viewport_ext_y: mapping_extent_magnitude(window_ext_y),
      current_pos_x: 0,
      current_pos_y: 0,
      objects: vec![None; metafile.header.number_of_objects as usize],
      current_font_height: 12,
      current_font_family: None,
      current_font_char_set: crate::wmf::WmfCharacterSet::Ansi.raw(),
      current_font_weight: 400,
      current_font_bold: false,
      current_font_italic: false,
      current_font_quality: crate::wmf::WmfFontQuality::Default.raw(),
      text_alignment: WmfTextAlignmentModeFlags::empty(),
      saved: Vec::new(),
      font_cache: RenderFontCache::load(),
      round_device_coordinates: wmf_external_canvas_size(metafile, external_header).is_some(),
    }
  }

  fn insert_object(&mut self, font: Option<WmfTextFont>) {
    let object = font.unwrap_or(WmfTextFont {
      height: 0,
      family: None,
      char_set: crate::wmf::WmfCharacterSet::Ansi.raw(),
      weight: 400,
      italic: false,
      quality: crate::wmf::WmfFontQuality::Default.raw(),
    });
    if let Some(slot) = self.objects.iter_mut().find(|slot| slot.is_none()) {
      *slot = Some(object);
    } else {
      self.objects.push(Some(object));
    }
  }

  fn select_object(&mut self, index: u16) {
    if let Some(Some(font)) = self.objects.get(index as usize)
      && font.height != 0
    {
      self.current_font_height = font.height.abs().max(7);
      self.current_font_family = font.family.clone();
      self.current_font_char_set = font.char_set;
      self.current_font_weight = font.weight;
      self.current_font_bold = font.weight > 400;
      self.current_font_italic = font.italic;
      self.current_font_quality = font.quality;
    }
  }

  fn save(&mut self) {
    self.saved.push(WmfTextSnapshot {
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      current_pos_x: self.current_pos_x,
      current_pos_y: self.current_pos_y,
      current_font_height: self.current_font_height,
      current_font_family: self.current_font_family.clone(),
      current_font_char_set: self.current_font_char_set,
      current_font_weight: self.current_font_weight,
      current_font_bold: self.current_font_bold,
      current_font_italic: self.current_font_italic,
      current_font_quality: self.current_font_quality,
      text_alignment: self.text_alignment,
    });
  }

  fn restore(&mut self) {
    let Some(snapshot) = self.saved.pop() else {
      return;
    };
    self.window_org_x = snapshot.window_org_x;
    self.window_org_y = snapshot.window_org_y;
    self.window_ext_x = snapshot.window_ext_x;
    self.window_ext_y = snapshot.window_ext_y;
    self.viewport_org_x = snapshot.viewport_org_x;
    self.viewport_org_y = snapshot.viewport_org_y;
    self.viewport_ext_x = snapshot.viewport_ext_x;
    self.viewport_ext_y = snapshot.viewport_ext_y;
    self.current_pos_x = snapshot.current_pos_x;
    self.current_pos_y = snapshot.current_pos_y;
    self.current_font_height = snapshot.current_font_height;
    self.current_font_family = snapshot.current_font_family;
    self.current_font_char_set = snapshot.current_font_char_set;
    self.current_font_weight = snapshot.current_font_weight;
    self.current_font_bold = snapshot.current_font_bold;
    self.current_font_italic = snapshot.current_font_italic;
    self.current_font_quality = snapshot.current_font_quality;
    self.text_alignment = snapshot.text_alignment;
  }

  fn scale_window(&mut self, value: crate::wmf::WmfScaleExtRecord) {
    self.window_ext_x = scale_wmf_extent(self.window_ext_x, value.x_num, value.x_denom);
    self.window_ext_y = scale_wmf_extent(self.window_ext_y, value.y_num, value.y_denom);
  }

  fn scale_viewport(&mut self, value: crate::wmf::WmfScaleExtRecord) {
    self.viewport_ext_x = scale_wmf_extent(self.viewport_ext_x, value.x_num, value.x_denom);
    self.viewport_ext_y = scale_wmf_extent(self.viewport_ext_y, value.y_num, value.y_denom);
  }

  fn text_run(
    &mut self,
    text: String,
    x: i16,
    y: i16,
    logical_advances: Option<&[i16]>,
  ) -> Option<MetafileTextRun> {
    if text.is_empty() {
      return None;
    }
    let scale_x = self.viewport_ext_x as f32 / self.window_ext_x as f32;
    let scale_y = self.viewport_ext_y as f32 / self.window_ext_y as f32;
    let logical_width = logical_advances.map(|values| {
      values.iter().fold(0i32, |total, advance| {
        total.saturating_add(i32::from(*advance))
      })
    });
    let update_current_position = self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::UPDATE_CP);
    let (reference_x, reference_y) = if update_current_position {
      (self.current_pos_x, self.current_pos_y)
    } else {
      (i32::from(x), i32::from(y))
    };
    let aligned_x = if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::CENTER)
    {
      reference_x.saturating_sub(logical_width.unwrap_or_default() / 2)
    } else if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::RIGHT)
    {
      reference_x.saturating_sub(logical_width.unwrap_or_default())
    } else {
      reference_x
    };
    let realize_coordinate = |value: f32| {
      if self.round_device_coordinates {
        value.round()
      } else {
        value
      }
    };
    let mapped_x = realize_coordinate(
      self.viewport_org_x as f32 + (aligned_x - self.window_org_x) as f32 * scale_x,
    );
    let mapped_reference_y = realize_coordinate(
      self.viewport_org_y as f32 + (reference_y - self.window_org_y) as f32 * scale_y,
    );
    let font = WmfTextFont {
      height: self.current_font_height,
      family: self.current_font_family.clone(),
      char_set: self.current_font_char_set,
      weight: self.current_font_weight,
      italic: self.current_font_italic,
      quality: self.current_font_quality,
    };
    // [MS-WMF] §2.1.2.3 defines TA_TOP against the realized font's
    // alignment box. GDI therefore advances by TEXTMETRIC.tmAscent, not by
    // the requested LOGFONT character height. Use the same realized-face
    // metrics as EMF extraction and raster playback; retain lfHeight only as
    // the no-font fallback inside `baseline_for_alignment`.
    let mapped_font_height = self.current_font_height.abs() as f32 * scale_y.abs();
    let continuous_baseline = self.font_cache.baseline_for_alignment(
      &font,
      mapped_font_height.max(1.0),
      mapped_reference_y,
      self.text_alignment,
    );
    // Windows exposes TEXTMETRIC ascent/descent in integer device pixels.
    // Keep the mapped reference coordinate intact, but realize the alignment
    // advance on that integer grid before normalizing it for a vector host.
    let alignment_advance = continuous_baseline - mapped_reference_y;
    let realized_advance = if alignment_advance.is_sign_negative() {
      alignment_advance.floor()
    } else {
      alignment_advance.ceil()
    };
    let mapped_y = mapped_reference_y + realized_advance;
    let advances = logical_advances.map(|values| {
      if self.round_device_coordinates {
        let values = values
          .iter()
          .map(|value| i32::from(*value))
          .collect::<Vec<_>>();
        cumulative_mapped_advances(&values, |logical_cumulative| {
          (logical_cumulative as f32 * scale_x).round()
        })
        .into_iter()
        .map(|advance| advance / self.natural_width)
        .collect::<Vec<_>>()
      } else {
        values
          .iter()
          .map(|advance| f32::from(*advance) * scale_x / self.natural_width)
          .collect::<Vec<_>>()
      }
    });
    let run = MetafileTextRun {
      text,
      x: mapped_x / self.natural_width,
      y: mapped_y / self.natural_height,
      font_size: Some(self.current_font_height.abs() as f32 * scale_y.abs() / self.natural_height),
      font_family: self.current_font_family.clone(),
      bold: self.current_font_bold,
      italic: self.current_font_italic,
      width: advances
        .as_ref()
        .map(|values| values.iter().copied().sum::<f32>().abs())
        .filter(|width| width.is_finite() && *width > 0.0),
      advances,
      requires_raster_backdrop: false,
    };
    if update_current_position && let Some(logical_width) = logical_width {
      self.current_pos_x = aligned_x.saturating_add(logical_width);
      self.current_pos_y = reference_y;
    }
    Some(run)
  }
}

#[derive(Clone, Copy, Debug)]
enum WmfSolidRectObject {
  Brush(Option<EmfColor>),
  Other,
}

#[derive(Clone, Debug)]
struct WmfSolidRectSnapshot {
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  current_solid_brush: Option<EmfColor>,
}

struct WmfSolidRectState {
  natural_width: f32,
  natural_height: f32,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  objects: Vec<Option<WmfSolidRectObject>>,
  current_solid_brush: Option<EmfColor>,
  saved: Vec<WmfSolidRectSnapshot>,
  round_device_coordinates: bool,
}

impl WmfSolidRectState {
  fn new(metafile: &WmfMetafileRef<'_>, external_header: Option<WmfExternalHeader>) -> Self {
    let (window_org_x, window_org_y, window_ext_x, window_ext_y) =
      wmf_initial_window(metafile, external_header);
    let natural_width = mapping_extent_magnitude(window_ext_x) as f32;
    let natural_height = mapping_extent_magnitude(window_ext_y) as f32;
    Self {
      natural_width,
      natural_height,
      window_org_x,
      window_org_y,
      window_ext_x: nonzero_mapping_extent(window_ext_x),
      window_ext_y: nonzero_mapping_extent(window_ext_y),
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: natural_width as i32,
      viewport_ext_y: natural_height as i32,
      objects: vec![None; metafile.header.number_of_objects as usize],
      current_solid_brush: None,
      saved: Vec::new(),
      round_device_coordinates: wmf_external_canvas_size(metafile, external_header).is_some(),
    }
  }

  fn insert_object(&mut self, object: WmfSolidRectObject) {
    if let Some(slot) = self.objects.iter_mut().find(|slot| slot.is_none()) {
      *slot = Some(object);
    } else {
      self.objects.push(Some(object));
    }
  }

  fn select_object(&mut self, index: u16) {
    if let Some(Some(WmfSolidRectObject::Brush(color))) = self.objects.get(index as usize).copied()
    {
      self.current_solid_brush = color;
    }
  }

  fn save(&mut self) {
    self.saved.push(WmfSolidRectSnapshot {
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      current_solid_brush: self.current_solid_brush,
    });
  }

  fn restore(&mut self) {
    let Some(saved) = self.saved.pop() else {
      return;
    };
    self.window_org_x = saved.window_org_x;
    self.window_org_y = saved.window_org_y;
    self.window_ext_x = saved.window_ext_x;
    self.window_ext_y = saved.window_ext_y;
    self.viewport_org_x = saved.viewport_org_x;
    self.viewport_org_y = saved.viewport_org_y;
    self.viewport_ext_x = saved.viewport_ext_x;
    self.viewport_ext_y = saved.viewport_ext_y;
    self.current_solid_brush = saved.current_solid_brush;
  }

  fn scale_window(&mut self, value: crate::wmf::WmfScaleExtRecord) {
    self.window_ext_x = scale_wmf_extent(self.window_ext_x, value.x_num, value.x_denom);
    self.window_ext_y = scale_wmf_extent(self.window_ext_y, value.y_num, value.y_denom);
  }

  fn scale_viewport(&mut self, value: crate::wmf::WmfScaleExtRecord) {
    self.viewport_ext_x = scale_wmf_extent(self.viewport_ext_x, value.x_num, value.x_denom);
    self.viewport_ext_y = scale_wmf_extent(self.viewport_ext_y, value.y_num, value.y_denom);
  }

  fn normalized_rect(
    &self,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
  ) -> Option<(f32, f32, f32, f32, bool, bool)> {
    let scale_x = self.viewport_ext_x as f32 / self.window_ext_x as f32;
    let scale_y = self.viewport_ext_y as f32 / self.window_ext_y as f32;
    let realize_coordinate = |value: f32| {
      if self.round_device_coordinates {
        value.round()
      } else {
        value
      }
    };
    let map_x = |value: i32| {
      realize_coordinate(self.viewport_org_x as f32 + (value - self.window_org_x) as f32 * scale_x)
    };
    let map_y = |value: i32| {
      realize_coordinate(self.viewport_org_y as f32 + (value - self.window_org_y) as f32 * scale_y)
    };
    let first_x = map_x(x);
    let second_x = map_x(x.saturating_add(width));
    let first_y = map_y(y);
    let second_y = map_y(y.saturating_add(height));
    let left = first_x.min(second_x) / self.natural_width;
    let top = first_y.min(second_y) / self.natural_height;
    let width = (first_x - second_x).abs() / self.natural_width;
    let height = (first_y - second_y).abs() / self.natural_height;
    (left.is_finite()
      && top.is_finite()
      && width.is_finite()
      && height.is_finite()
      && width > 0.0
      && height > 0.0)
      .then_some((
        left,
        top,
        width,
        height,
        second_x < first_x,
        second_y < first_y,
      ))
  }

  fn solid_rect(&self, value: crate::wmf::WmfPatBltRecord) -> Option<MetafileSolidRect> {
    if value.raster_operation_code() != WmfTernaryRasterOperationCode::PATCOPY {
      return None;
    }
    let color = self.current_solid_brush?;
    let (x, y, width, height, _, _) = self.normalized_rect(
      i32::from(value.x_left),
      i32::from(value.y_left),
      i32::from(value.width),
      i32::from(value.height),
    )?;
    Some(MetafileSolidRect {
      x,
      y,
      width,
      height,
      color: [color.r, color.g, color.b],
    })
  }
}

fn extract_wmf_solid_rects(
  data: &[u8],
  external_header: Option<WmfExternalHeader>,
) -> Vec<MetafileSolidRect> {
  let Ok(metafile) = WmfMetafileRef::from_bytes(data) else {
    return Vec::new();
  };
  let mut state = WmfSolidRectState::new(&metafile, external_header);
  let mut rects = Vec::new();
  for record in metafile.records() {
    let Ok(record) = record.parse_data() else {
      continue;
    };
    match record {
      WmfRecordData::Eof(_) => break,
      WmfRecordData::SaveDc => state.save(),
      WmfRecordData::RestoreDc(_) => state.restore(),
      WmfRecordData::SetWindowOrg(value) => {
        state.window_org_x = i32::from(value.x);
        state.window_org_y = i32::from(value.y);
      }
      WmfRecordData::SetWindowExt(value) => {
        state.window_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.window_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::SetViewportOrg(value) => {
        state.viewport_org_x = i32::from(value.x);
        state.viewport_org_y = i32::from(value.y);
      }
      WmfRecordData::SetViewportExt(value) => {
        state.viewport_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.viewport_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::OffsetWindowOrg(value) => {
        state.window_org_x += i32::from(value.x);
        state.window_org_y += i32::from(value.y);
      }
      WmfRecordData::OffsetViewportOrg(value) => {
        state.viewport_org_x += i32::from(value.x);
        state.viewport_org_y += i32::from(value.y);
      }
      WmfRecordData::ScaleWindowExt(value) => state.scale_window(value),
      WmfRecordData::ScaleViewportExt(value) => state.scale_viewport(value),
      WmfRecordData::CreateBrushIndirect(value) => {
        let color = (value.brush_style_kind() == Some(WmfBrushStyle::Solid))
          .then(|| color_ref_to_emf(value.color_ref));
        let object = WmfSolidRectObject::Brush(color);
        state.insert_object(object);
      }
      WmfRecordData::CreatePenIndirect(_)
      | WmfRecordData::CreateFontIndirect(_)
      | WmfRecordData::CreatePalette(_)
      | WmfRecordData::CreateRegion(_) => {
        state.insert_object(WmfSolidRectObject::Other);
      }
      WmfRecordData::CreatePatternBrush(_) | WmfRecordData::DibCreatePatternBrush(_) => {
        state.insert_object(WmfSolidRectObject::Brush(None));
      }
      WmfRecordData::SelectObject(value) => state.select_object(value.index),
      WmfRecordData::DeleteObject(value) => {
        if let Some(slot) = state.objects.get_mut(value.index as usize) {
          *slot = None;
        }
      }
      WmfRecordData::PatBlt(value) => {
        if let Some(rect) = state.solid_rect(value) {
          rects.push(rect);
        }
      }
      _ => {}
    }
  }
  rects
}

struct WmfMaskedBitmapPair {
  source: RasterPixels,
  mask: RasterPixels,
  alpha_is_mask_value: bool,
}

fn same_wmf_dib_geometry(
  first: &crate::wmf::WmfDibStretchBltRecord,
  second: &crate::wmf::WmfDibStretchBltRecord,
) -> bool {
  first.x_dest == second.x_dest
    && first.y_dest == second.y_dest
    && first.dest_width == second.dest_width
    && first.dest_height == second.dest_height
    && first.x_src == second.x_src
    && first.y_src == second.y_src
    && first.src_width == second.src_width
    && first.src_height == second.src_height
}

fn full_wmf_dib_source(value: &crate::wmf::WmfDibStretchBltRecord) -> Option<RasterPixels> {
  let bytes = value.target.source_bytes()?;
  let image = packed_dib_to_rgb(bytes, DibColorUsage::RgbColors)
    .ok()
    .flatten()?;
  // Keep partial source rectangles in the ordinary raster replay until their
  // bottom-up DIB coordinate and mirroring semantics can be represented by a
  // standalone layer without ambiguity. The transparent previews emitted by
  // Office use the complete DIB here.
  let source_width = i32::from(value.src_width).unsigned_abs() as usize;
  let source_height = i32::from(value.src_height).unsigned_abs() as usize;
  (value.x_src == 0
    && value.y_src == 0
    && source_width == image.width
    && source_height == image.height)
    .then_some(image)
}

fn wmf_masked_bitmap_pair(
  first: &crate::wmf::WmfDibStretchBltRecord,
  second: &crate::wmf::WmfDibStretchBltRecord,
) -> Option<WmfMaskedBitmapPair> {
  if !same_wmf_dib_geometry(first, second) {
    return None;
  }
  let alpha_is_mask_value = match (
    first.raster_operation_code(),
    second.raster_operation_code(),
  ) {
    (WmfTernaryRasterOperationCode::SRCPAINT, WmfTernaryRasterOperationCode::SRCAND) => true,
    (
      WmfTernaryRasterOperationCode::SRCAND,
      WmfTernaryRasterOperationCode::SRCPAINT | WmfTernaryRasterOperationCode::SRCINVERT,
    ) => false,
    _ => return None,
  };
  let mask = full_wmf_dib_source(first)?;
  if !is_binary_monochrome_raster(&mask) {
    return None;
  }
  let source = full_wmf_dib_source(second)?;
  if source.width != mask.width || source.height != mask.height {
    return None;
  }
  Some(WmfMaskedBitmapPair {
    source,
    mask,
    alpha_is_mask_value,
  })
}

fn wmf_masked_bitmap_layer(
  state: &WmfSolidRectState,
  first: &crate::wmf::WmfDibStretchBltRecord,
  second: &crate::wmf::WmfDibStretchBltRecord,
) -> Option<MetafileBitmapLayer> {
  let pair = wmf_masked_bitmap_pair(first, second)?;
  let (x, y, width, height, mapped_flip_horizontal, mapped_flip_vertical) = state.normalized_rect(
    i32::from(first.x_dest),
    i32::from(first.y_dest),
    i32::from(first.dest_width),
    i32::from(first.dest_height),
  )?;
  let mut rgba = Vec::with_capacity(pair.source.width * pair.source.height * BGRA_BYTES_PER_PIXEL);
  for (source, mask) in pair
    .source
    .rgb
    .chunks_exact(RGB_BYTES_PER_PIXEL)
    .zip(pair.mask.rgb.chunks_exact(RGB_BYTES_PER_PIXEL))
  {
    rgba.extend_from_slice(source);
    rgba.push(if pair.alpha_is_mask_value {
      mask[0]
    } else {
      u8::MAX - mask[0]
    });
  }
  let data = rgba_to_png(&rgba, pair.source.width as u32, pair.source.height as u32).ok()?;
  Some(MetafileBitmapLayer {
    data,
    content_type: "image/png",
    x,
    y,
    width,
    height,
    flip_horizontal: mapped_flip_horizontal ^ first.src_width.is_negative(),
    flip_vertical: mapped_flip_vertical ^ first.src_height.is_negative(),
  })
}

fn wmf_copy_bitmap_layer(
  state: &WmfSolidRectState,
  value: &crate::wmf::WmfDibStretchBltRecord,
) -> Option<MetafileBitmapLayer> {
  if value.raster_operation_code() != WmfTernaryRasterOperationCode::SRCCOPY {
    return None;
  }
  let source = full_wmf_dib_source(value)?;
  let (x, y, width, height, mapped_flip_horizontal, mapped_flip_vertical) = state.normalized_rect(
    i32::from(value.x_dest),
    i32::from(value.y_dest),
    i32::from(value.dest_width),
    i32::from(value.dest_height),
  )?;
  let data = rgb_to_png(&source.rgb, source.width as u32, source.height as u32).ok()?;
  Some(MetafileBitmapLayer {
    data,
    content_type: "image/png",
    x,
    y,
    width,
    height,
    flip_horizontal: mapped_flip_horizontal ^ value.src_width.is_negative(),
    flip_vertical: mapped_flip_vertical ^ value.src_height.is_negative(),
  })
}

fn extract_wmf_bitmap_layers(
  data: &[u8],
  external_header: Option<WmfExternalHeader>,
) -> Vec<MetafileBitmapLayer> {
  let Ok(metafile) = WmfMetafileRef::from_bytes(data) else {
    return Vec::new();
  };
  let mut state = WmfSolidRectState::new(&metafile, external_header);
  let mut layers = Vec::new();
  let mut records = metafile.records().peekable();
  while let Some(record) = records.next() {
    let Ok(record) = record.parse_data() else {
      continue;
    };
    match record {
      WmfRecordData::Eof(_) => break,
      WmfRecordData::SaveDc => state.save(),
      WmfRecordData::RestoreDc(_) => state.restore(),
      WmfRecordData::SetWindowOrg(value) => {
        state.window_org_x = i32::from(value.x);
        state.window_org_y = i32::from(value.y);
      }
      WmfRecordData::SetWindowExt(value) => {
        state.window_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.window_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::SetViewportOrg(value) => {
        state.viewport_org_x = i32::from(value.x);
        state.viewport_org_y = i32::from(value.y);
      }
      WmfRecordData::SetViewportExt(value) => {
        state.viewport_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.viewport_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::OffsetWindowOrg(value) => {
        state.window_org_x += i32::from(value.x);
        state.window_org_y += i32::from(value.y);
      }
      WmfRecordData::OffsetViewportOrg(value) => {
        state.viewport_org_x += i32::from(value.x);
        state.viewport_org_y += i32::from(value.y);
      }
      WmfRecordData::ScaleWindowExt(value) => state.scale_window(value),
      WmfRecordData::ScaleViewportExt(value) => state.scale_viewport(value),
      WmfRecordData::DibStretchBlt(first) => {
        let next = records
          .peek()
          .copied()
          .and_then(|record| record.parse_data().ok());
        if let Some(WmfRecordData::DibStretchBlt(second)) = next
          && let Some(layer) = wmf_masked_bitmap_layer(&state, &first, &second)
        {
          records.next();
          layers.push(layer);
          continue;
        }
        if let Some(layer) = wmf_copy_bitmap_layer(&state, &first) {
          layers.push(layer);
        }
      }
      _ => {}
    }
  }
  layers
}

fn scale_wmf_extent(extent: i32, numerator: i16, denominator: i16) -> i32 {
  if denominator == 0 {
    return extent;
  }
  ((i64::from(extent) * i64::from(numerator)) / i64::from(denominator))
    .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn nonzero_mapping_extent(extent: i32) -> i32 {
  // [MS-EMF] §§2.3.11.28/30 and [MS-WMF] §§2.3.5.28/30 store viewport and
  // window extents as signed integers. Their sign participates in VExt/WExt
  // and can reverse an axis; it is not a canvas-size magnitude.
  if extent == 0 { 1 } else { extent }
}

fn emf_mapping_extents_are_variable(map_mode: EmrMapMode) -> bool {
  matches!(map_mode, EmrMapMode::Isotropic | EmrMapMode::Anisotropic)
}

fn emf_window_viewport_scale(
  map_mode: EmrMapMode,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
) -> (f32, f32) {
  if emf_mapping_extents_are_variable(map_mode) {
    (
      viewport_ext_x as f32 / nonzero_mapping_extent(window_ext_x) as f32,
      viewport_ext_y as f32 / nonzero_mapping_extent(window_ext_y) as f32,
    )
  } else {
    // [MS-EMF] 2.1.21 and 2.3.11.7-8: fixed-scale mapping modes do
    // not accept window/viewport extent changes. In particular, the default
    // MM_TEXT maps one logical unit to one device pixel. Physical-unit fixed
    // modes have their own page-to-device conversion, independent of these
    // extents.
    (1.0, 1.0)
  }
}

fn mapping_extent_magnitude(extent: i32) -> i32 {
  extent.saturating_abs().max(1)
}

fn wmf_text_font(value: &crate::wmf::WmfFontObject) -> WmfTextFont {
  let face_name = &value.face_name[..usize::from(value.face_name_bytes)];
  let face_name = &face_name[..face_name
    .iter()
    .position(|byte| *byte == 0)
    .unwrap_or(face_name.len())];
  // Real-world META_CREATEFONTINDIRECT records use the selected LOGFONT
  // charset for both their text and face-name bytes. In particular, Office's
  // GB2312 records store localized names such as `宋体` in code page 936.
  // LibreOffice's WMF reader follows the same charset-first conversion. A
  // Windows-1252-first probe cannot detect this: every DBCS byte is valid in
  // that single-byte encoding and becomes a different, non-existent family.
  // Keep the spec-compatible ANSI fallback for unknown vendor charsets.
  let family = crate::string::SdkEncoding::WmfCharset(value.char_set)
    .decode(face_name)
    .or_else(|_| crate::string::SdkEncoding::Windows1252.decode(face_name))
    .ok()
    .map(|family| family.trim().to_string())
    .filter(|family| !family.is_empty());
  let char_set = if family.as_deref().is_some_and(|family| {
    family.eq_ignore_ascii_case("Symbol") || family.eq_ignore_ascii_case("MT Extra")
  }) {
    crate::wmf::WmfCharacterSet::Symbol.raw()
  } else {
    value.char_set
  };
  WmfTextFont {
    height: i32::from(value.height),
    family,
    char_set,
    weight: value.weight.max(0) as u16,
    italic: value.italic != 0,
    quality: value.quality,
  }
}

fn extract_wmf_text_runs(
  data: &[u8],
  external_header: Option<WmfExternalHeader>,
) -> Vec<MetafileTextRun> {
  let Ok(metafile) = WmfMetafileRef::from_bytes(data) else {
    return Vec::new();
  };
  let mut state = WmfTextState::new(&metafile, external_header);
  let mut runs = Vec::new();
  let mut requires_raster_backdrop = false;
  for record in metafile.records() {
    let Ok(record) = record.parse_data() else {
      continue;
    };
    match record {
      WmfRecordData::Eof(_) => break,
      WmfRecordData::SaveDc => state.save(),
      WmfRecordData::RestoreDc(_) => state.restore(),
      WmfRecordData::SetWindowOrg(value) => {
        state.window_org_x = i32::from(value.x);
        state.window_org_y = i32::from(value.y);
      }
      WmfRecordData::SetWindowExt(value) => {
        state.window_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.window_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::SetViewportOrg(value) => {
        state.viewport_org_x = i32::from(value.x);
        state.viewport_org_y = i32::from(value.y);
      }
      WmfRecordData::SetViewportExt(value) => {
        state.viewport_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.viewport_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::OffsetWindowOrg(value) => {
        state.window_org_x += i32::from(value.x);
        state.window_org_y += i32::from(value.y);
      }
      WmfRecordData::OffsetViewportOrg(value) => {
        state.viewport_org_x += i32::from(value.x);
        state.viewport_org_y += i32::from(value.y);
      }
      WmfRecordData::ScaleWindowExt(value) => state.scale_window(value),
      WmfRecordData::ScaleViewportExt(value) => state.scale_viewport(value),
      WmfRecordData::SetTextAlign(value) => {
        state.text_alignment = value.text_alignment_flags();
      }
      WmfRecordData::MoveTo(value) => {
        state.current_pos_x = i32::from(value.x);
        state.current_pos_y = i32::from(value.y);
      }
      WmfRecordData::CreateFontIndirect(value) => {
        state.insert_object(Some(wmf_text_font(&value)));
      }
      WmfRecordData::CreatePenIndirect(_)
      | WmfRecordData::CreateBrushIndirect(_)
      | WmfRecordData::CreatePalette(_)
      | WmfRecordData::CreatePatternBrush(_)
      | WmfRecordData::CreateRegion(_)
      | WmfRecordData::DibCreatePatternBrush(_) => state.insert_object(None),
      WmfRecordData::SelectObject(value) => state.select_object(value.index),
      WmfRecordData::DeleteObject(value) => {
        if let Some(slot) = state.objects.get_mut(value.index as usize) {
          *slot = None;
        }
      }
      WmfRecordData::TextOut(value) => {
        let text = decode_wmf_text(&value.string, state.current_font_char_set);
        if let Some(mut run) = state.text_run(text, value.x_start, value.y_start, None) {
          run.requires_raster_backdrop = requires_raster_backdrop;
          runs.push(run);
        }
      }
      WmfRecordData::ExtTextOut(value) => {
        let text = decode_wmf_text(&value.string, state.current_font_char_set);
        let advances = (!value.dx.is_empty()).then_some(value.dx.as_slice());
        if let Some(mut run) = state.text_run(text, value.x, value.y, advances) {
          run.requires_raster_backdrop = requires_raster_backdrop;
          runs.push(run);
        }
      }
      WmfRecordData::BitBlt(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      WmfRecordData::DibBitBlt(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      WmfRecordData::StretchBlt(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      WmfRecordData::DibStretchBlt(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      WmfRecordData::StretchDib(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      WmfRecordData::PatBlt(value) => {
        requires_raster_backdrop |= value.ternary_raster_operation().uses_destination();
      }
      _ => {}
    }
  }
  runs
}

pub fn looks_like_metafile(data: &[u8], content_type: Option<&str>) -> bool {
  matches!(
    content_type,
    Some("image/x-wmf" | "image/wmf" | "image/x-emf" | "image/emf" | "application/x-msmetafile")
  ) || is_emf(data)
    || crate::wmf::looks_like_wmf(data)
}

fn decode_emf_as_raster(
  data: &[u8],
  options: RenderOptions,
  force_vector_replay: bool,
  text_surface: GdiTextSurface,
) -> Result<Option<DecodedMetafile>, String> {
  let Some(mut pos) = emf_header_record_size(data) else {
    return Ok(None);
  };

  let mut bitmap_record = None;
  let mut bitmap_count = 0usize;
  let mut needs_vector_replay = false;

  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let record_type = read_u32(data, pos)?;
    let record_size = read_u32(data, pos + 4)? as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      return Err(format!(
        "invalid EMF record at offset {pos}: type=0x{record_type:08x} size={record_size}"
      ));
    }
    if matches!(
      record_type,
      EMR_BIT_BLT | EMR_STRETCH_BLT | EMR_SET_DIBITS_TO_DEVICE | EMR_STRETCH_DIBITS
    ) {
      bitmap_count += 1;
      bitmap_record = Some((record_type, pos, record_size));
      // BITBLT and STRETCHBLT can depend on the existing destination through
      // their ternary raster operation, even when they are the only bitmap
      // record in the metafile.
      if bitmap_count > 1 || matches!(record_type, EMR_BIT_BLT | EMR_STRETCH_BLT) {
        needs_vector_replay = true;
      }
    } else if emf_record_needs_vector_replay(record_type) {
      needs_vector_replay = true;
    }

    pos += record_size;
    if record_type == EMR_EOF {
      break;
    }
  }

  if needs_vector_replay || force_vector_replay {
    return decode_vector_emf_as_png(data, options, text_surface).map(Some);
  }

  let (record_type, record_offset, record_size) = match bitmap_record {
    Some(record) => record,
    None => return decode_vector_emf_as_png(data, options, text_surface).map(Some),
  };
  decode_bitmap_record_as_raster(data, record_type, record_offset, record_size).map(Some)
}

fn emf_record_needs_vector_replay(record_type: u32) -> bool {
  matches!(
    record_type,
    EMR_POLYBEZIER
      | EMR_POLYGON
      | EMR_POLYLINE
      | EMR_POLYBEZIER_TO
      | EMR_POLYLINE_TO
      | EMR_POLYPOLYLINE
      | EMR_POLYPOLYGON
      | EMR_SET_PIXEL_V
      | EMR_MOVE_TO_EX
      | EMR_ELLIPSE
      | EMR_RECTANGLE
      | EMR_ROUND_RECT
      | EMR_ARC
      | EMR_CHORD
      | EMR_PIE
      | EMR_LINE_TO
      | EMR_COMMENT
      | EMR_EXT_TEXTOUT_A
      | EMR_EXT_TEXTOUT_W
      | EMR_POLYBEZIER16
      | EMR_POLYGON16
      | EMR_POLYLINE16
      | EMR_POLYBEZIER_TO16
      | EMR_POLYLINE_TO16
      | EMR_POLYPOLYLINE16
      | EMR_POLYPOLYGON16
      | EMR_ALPHA_BLEND
  )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EmfColor {
  r: u8,
  g: u8,
  b: u8,
}

impl EmfColor {
  fn not(self) -> Self {
    Self {
      r: !self.r,
      g: !self.g,
      b: !self.b,
    }
  }

  fn and(self, other: Self) -> Self {
    Self {
      r: self.r & other.r,
      g: self.g & other.g,
      b: self.b & other.b,
    }
  }

  fn or(self, other: Self) -> Self {
    Self {
      r: self.r | other.r,
      g: self.g | other.g,
      b: self.b | other.b,
    }
  }

  fn xor(self, other: Self) -> Self {
    Self {
      r: self.r ^ other.r,
      g: self.g ^ other.g,
      b: self.b ^ other.b,
    }
  }
}

#[derive(Clone, Copy, Debug)]
struct EmfPoint {
  x: i32,
  y: i32,
}

#[derive(Clone, Copy, Debug)]
struct EmfTransform {
  m11: f32,
  m12: f32,
  m21: f32,
  m22: f32,
  dx: f32,
  dy: f32,
}

impl EmfTransform {
  fn identity() -> Self {
    Self {
      m11: 1.0,
      m12: 0.0,
      m21: 0.0,
      m22: 1.0,
      dx: 0.0,
      dy: 0.0,
    }
  }

  fn apply(self, point: EmfPoint) -> (f32, f32) {
    let x = point.x as f32;
    let y = point.y as f32;
    (
      x * self.m11 + y * self.m21 + self.dx,
      x * self.m12 + y * self.m22 + self.dy,
    )
  }

  fn multiply(self, other: Self) -> Self {
    Self {
      m11: self.m11 * other.m11 + self.m12 * other.m21,
      m12: self.m11 * other.m12 + self.m12 * other.m22,
      m21: self.m21 * other.m11 + self.m22 * other.m21,
      m22: self.m21 * other.m12 + self.m22 * other.m22,
      dx: self.dx * other.m11 + self.dy * other.m21 + other.dx,
      dy: self.dx * other.m12 + self.dy * other.m22 + other.dy,
    }
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EmfPenWidthSpace {
  /// The width has already been realized on the output device.
  Device,
  /// A WMF logical-object width, mapped as an x-scalar when the object is
  /// created. [MS-WMF] 3.1.4.2 explicitly excludes the y-scalar.
  LogicalX,
  /// An EMF+ `UnitWorld` width, transformed by the active world-to-device
  /// transform when the pen is used.
  World,
}

#[derive(Clone, Copy, Debug)]
struct EmfPen {
  color: EmfColor,
  alpha: u8,
  width: usize,
  width_space: EmfPenWidthSpace,
}

fn wmf_pen_width(width: i16) -> (usize, EmfPenWidthSpace) {
  let logical_width = i32::from(width).unsigned_abs() as usize;
  if logical_width == 0 {
    (1, EmfPenWidthSpace::Device)
  } else {
    (logical_width, EmfPenWidthSpace::LogicalX)
  }
}

fn emf_pen_from_style(style: u32, pen: EmfPen) -> Option<EmfPen> {
  (EmrPenLineStyle::from_raw(style & 0x0000_000F) != Some(EmrPenLineStyle::Null)).then_some(pen)
}

#[derive(Clone, Debug)]
struct EmfFont {
  height: i32,
  family: Option<String>,
  char_set: u8,
  weight: u16,
  italic: bool,
  quality: u8,
}

#[derive(Clone, Copy)]
struct EmfTextSnapshot {
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  current_pos: EmfPoint,
  current_font: Option<u32>,
  text_alignment: WmfTextAlignmentModeFlags,
}

struct EmfTextState {
  width: usize,
  height: usize,
  playback_origin_x: f32,
  playback_origin_y: f32,
  playback_scale_x: f32,
  playback_scale_y: f32,
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  current_pos: EmfPoint,
  fonts: std::collections::HashMap<u32, EmfFont>,
  current_font: Option<u32>,
  text_alignment: WmfTextAlignmentModeFlags,
  saved: Vec<EmfTextSnapshot>,
  font_cache: RenderFontCache,
}

impl EmfTextState {
  fn new(data: &[u8]) -> Result<Self, String> {
    let geometry = emf_playback_geometry(data)?;

    Ok(Self {
      width: geometry.width,
      height: geometry.height,
      playback_origin_x: geometry.origin_x,
      playback_origin_y: geometry.origin_y,
      playback_scale_x: geometry.scale_x,
      playback_scale_y: geometry.scale_y,
      map_mode: EmrMapMode::Text,
      window_org_x: 0,
      window_org_y: 0,
      window_ext_x: geometry.width as i32,
      window_ext_y: geometry.height as i32,
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: geometry.width as i32,
      viewport_ext_y: geometry.height as i32,
      world_transform: EmfTransform::identity(),
      current_pos: EmfPoint { x: 0, y: 0 },
      fonts: std::collections::HashMap::new(),
      current_font: None,
      text_alignment: WmfTextAlignmentModeFlags::empty(),
      saved: Vec::new(),
      font_cache: RenderFontCache::load(),
    })
  }

  fn save(&mut self) {
    self.saved.push(EmfTextSnapshot {
      map_mode: self.map_mode,
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      world_transform: self.world_transform,
      current_pos: self.current_pos,
      current_font: self.current_font,
      text_alignment: self.text_alignment,
    });
  }

  fn restore(&mut self) {
    let Some(snapshot) = self.saved.pop() else {
      return;
    };
    self.map_mode = snapshot.map_mode;
    self.window_org_x = snapshot.window_org_x;
    self.window_org_y = snapshot.window_org_y;
    self.window_ext_x = snapshot.window_ext_x;
    self.window_ext_y = snapshot.window_ext_y;
    self.viewport_org_x = snapshot.viewport_org_x;
    self.viewport_org_y = snapshot.viewport_org_y;
    self.viewport_ext_x = snapshot.viewport_ext_x;
    self.viewport_ext_y = snapshot.viewport_ext_y;
    self.world_transform = snapshot.world_transform;
    self.current_pos = snapshot.current_pos;
    self.current_font = snapshot.current_font;
    self.text_alignment = snapshot.text_alignment;
  }

  fn map_point(&self, point: EmfPoint) -> (f32, f32) {
    let (x, y) = self.world_transform.apply(point);
    let (scale_x, scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    (
      (self.viewport_org_x as f32 + (x - self.window_org_x as f32) * scale_x
        - self.playback_origin_x)
        * self.playback_scale_x,
      (self.viewport_org_y as f32 + (y - self.window_org_y as f32) * scale_y
        - self.playback_origin_y)
        * self.playback_scale_y,
    )
  }

  fn map_height(&self, height: i32) -> f32 {
    let (_, y0) = self.map_point(EmfPoint { x: 0, y: 0 });
    let (_, y1) = self.map_point(EmfPoint {
      x: 0,
      y: height.abs(),
    });
    (y1 - y0).abs()
  }

  fn map_horizontal_distance(&self, logical_width: i64) -> f32 {
    let width = logical_width as f32;
    let (scale_x, scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    let x = width * self.world_transform.m11 * scale_x * self.playback_scale_x;
    let y = width * self.world_transform.m12 * scale_y * self.playback_scale_y;
    x.hypot(y)
  }

  fn text_run(
    &mut self,
    data: &[u8],
    record_offset: usize,
    record_size: usize,
    text: String,
  ) -> Option<MetafileTextRun> {
    let text_record = ext_text_record(data, record_offset, record_size)?;
    let logical_advances = ext_text_advances(data, record_offset, record_size, text_record);
    let logical_displacement = ext_text_displacement(data, record_offset, record_size, text_record);
    let logical_width = logical_displacement.map(|displacement| displacement.x);
    let update_current_position = self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::UPDATE_CP);
    let reference = if update_current_position {
      self.current_pos
    } else {
      EmfPoint {
        x: text_record.x,
        y: text_record.y,
      }
    };
    let aligned_x = if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::CENTER)
    {
      reference
        .x
        .saturating_sub(logical_width.unwrap_or_default() / 2)
    } else if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::RIGHT)
    {
      reference
        .x
        .saturating_sub(logical_width.unwrap_or_default())
    } else {
      reference.x
    };
    let (x, reference_y) = self.map_point(EmfPoint {
      x: aligned_x,
      y: reference.y,
    });
    let selected_font = self
      .current_font
      .and_then(|id| self.fonts.get(&id))
      .cloned();
    let current_font = selected_font
      .as_ref()
      .map(|font| WmfTextFont {
        height: font.height,
        family: font.family.clone(),
        char_set: font.char_set,
        weight: font.weight,
        italic: font.italic,
        quality: font.quality,
      })
      .unwrap_or(WmfTextFont {
        height: 12,
        family: None,
        char_set: 0,
        weight: 400,
        italic: false,
        quality: crate::wmf::WmfFontQuality::Default.raw(),
      });
    let font_size = self.map_height(current_font.height);
    // [MS-EMF] 2.3.11.25 and 2.3.5 define these as reference coordinates.
    // Their meaning comes from EMR_SETTEXTALIGN, so semantic text must use
    // the same aligned origin and realized-font baseline as vector replay.
    let y = self.font_cache.baseline_for_alignment(
      &current_font,
      font_size.round().max(1.0),
      reference_y.round(),
      self.text_alignment,
    );
    let advances = logical_advances.as_deref().map(|values| {
      cumulative_mapped_advances(values, |logical_cumulative| {
        self.map_horizontal_distance(logical_cumulative) / self.width.max(1) as f32
      })
    });
    let run = MetafileTextRun {
      text,
      x: x / self.width.max(1) as f32,
      y: y / self.height.max(1) as f32,
      font_size: selected_font
        .as_ref()
        .map(|_| font_size / self.height.max(1) as f32),
      font_family: self
        .current_font
        .and_then(|id| self.fonts.get(&id))
        .and_then(|font| font.family.clone()),
      bold: self
        .current_font
        .and_then(|id| self.fonts.get(&id))
        .is_some_and(|font| font.weight > 400),
      italic: self
        .current_font
        .and_then(|id| self.fonts.get(&id))
        .is_some_and(|font| font.italic),
      // [MS-EMF] §2.2.5 defines Dx as the logical spacing between
      // consecutive character-cell origins. Map that logical distance
      // through the current page/world transform, then normalize it against
      // Header.Frame's playback surface. Header.Bounds encloses only marks;
      // using it as the canvas makes identical text wider whenever a
      // metafile happens to have tighter ink bounds.
      width: logical_width
        .map(|width| self.map_horizontal_distance(i64::from(width)) / self.width.max(1) as f32)
        .filter(|width| width.is_finite() && *width > 0.0),
      advances,
      requires_raster_backdrop: false,
    };
    if update_current_position && let Some(displacement) = logical_displacement {
      // LibreOffice MtfTools::DrawText follows GDI here: horizontal
      // alignment first shifts the text rectangle, then TA_UPDATECP moves to
      // that rectangle's final authored character-cell origin.
      self.current_pos = EmfPoint {
        x: aligned_x.saturating_add(displacement.x),
        y: reference.y.saturating_add(displacement.y),
      };
    }
    Some(run)
  }
}

struct EmfVectorState {
  width: usize,
  height: usize,
  natural_width: usize,
  natural_height: usize,
  playback_origin_x: f32,
  playback_origin_y: f32,
  playback_scale_x: f32,
  playback_scale_y: f32,
  output_scale_x: f32,
  output_scale_y: f32,
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  emf_plus_page_unit: EmfPlusUnitType,
  emf_plus_page_scale: f32,
  emf_plus_logical_dpi_x: f32,
  emf_plus_logical_dpi_y: f32,
  emf_plus_video_display: bool,
  brush_colors: std::collections::HashMap<u32, EmfColor>,
  solid_brushes: std::collections::HashSet<u32>,
  pens: std::collections::HashMap<u32, Option<EmfPen>>,
  fonts: std::collections::HashMap<u32, EmfFont>,
  current_brush: Option<EmfColor>,
  current_brush_is_solid: bool,
  current_pen: Option<EmfPen>,
  current_font: Option<u32>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  binary_raster_operation: WmfBinaryRasterOperation,
  text_alignment: WmfTextAlignmentModeFlags,
  clip_rect: Option<(i32, i32, i32, i32)>,
  clip_mask: Option<Vec<bool>>,
  saved_states: Vec<EmfVectorSnapshot>,
  emf_plus_saved_states: Vec<(u32, EmfVectorSnapshot)>,
  emf_plus_containers: Vec<(u32, EmfVectorSnapshot)>,
  emf_plus_objects: Vec<Option<EmfPlusRenderObject>>,
  emf_plus_object_assembler: EmfPlusObjectAssembler,
  font_cache: RenderFontCache,
  text_surface: GdiTextSurface,
  suppress_text: bool,
  rgb: Vec<u8>,
}

#[derive(Clone, Debug)]
struct EmfVectorSnapshot {
  map_mode: EmrMapMode,
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  world_transform: EmfTransform,
  emf_plus_page_unit: EmfPlusUnitType,
  emf_plus_page_scale: f32,
  current_brush: Option<EmfColor>,
  current_brush_is_solid: bool,
  current_pen: Option<EmfPen>,
  current_font: Option<u32>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  binary_raster_operation: WmfBinaryRasterOperation,
  text_alignment: WmfTextAlignmentModeFlags,
  clip_rect: Option<(i32, i32, i32, i32)>,
  clip_mask: Option<Vec<bool>>,
}

struct EmfDeviceContextBridge {
  snapshot: EmfVectorSnapshot,
  saved_states: Vec<EmfVectorSnapshot>,
  brush_colors: std::collections::HashMap<u32, EmfColor>,
  solid_brushes: std::collections::HashSet<u32>,
  pens: std::collections::HashMap<u32, Option<EmfPen>>,
  fonts: std::collections::HashMap<u32, EmfFont>,
}

#[derive(Clone, Debug)]
enum EmfPlusRenderObject {
  Brush(Option<EmfPlusRenderBrush>),
  Pen(Option<EmfPen>),
  Path(Vec<EmfPoint>),
  Region(EmfPlusRenderRegion),
  Image(RasterPixels),
  Font(EmfPlusFontObject),
  Unsupported,
}

#[derive(Clone, Debug)]
enum EmfPlusRenderRegion {
  Empty,
  Infinite,
  Polygon(Vec<EmfPoint>),
  Combine {
    mode: u8,
    left: Box<EmfPlusRenderRegion>,
    right: Box<EmfPlusRenderRegion>,
  },
}

#[derive(Clone, Debug)]
enum EmfPlusRenderBrush {
  Solid(EmfPlusRenderColor),
  Hatch {
    fore: EmfPlusRenderColor,
    back: EmfPlusRenderColor,
    style: u32,
  },
  LinearGradient {
    rect: (f32, f32, f32, f32),
    start: EmfPlusRenderColor,
    end: EmfPlusRenderColor,
  },
  PathGradient {
    center: (f32, f32),
    center_color: EmfPlusRenderColor,
    surround: EmfPlusRenderColor,
  },
  Texture(RasterPixels),
}

#[derive(Clone, Copy, Debug)]
struct EmfPlusRenderColor {
  color: EmfColor,
  alpha: u8,
}

impl EmfPlusRenderBrush {
  fn representative_color(&self) -> EmfPlusRenderColor {
    match self {
      Self::Solid(color) => *color,
      Self::Hatch { fore, .. } => *fore,
      Self::LinearGradient { start, end, .. } => average_emf_plus_color(*start, *end),
      Self::PathGradient {
        center_color,
        surround,
        ..
      } => average_emf_plus_color(*center_color, *surround),
      Self::Texture(image) => EmfPlusRenderColor {
        color: average_image_color(image),
        alpha: u8::MAX,
      },
    }
  }

  fn color_at(&self, x: i32, y: i32) -> EmfPlusRenderColor {
    match self {
      Self::Solid(color) => *color,
      Self::Hatch { fore, back, style } => {
        if EmfPlusHatchStyle::from_raw(*style).is_some_and(|style| style.is_foreground(x, y)) {
          *fore
        } else {
          *back
        }
      }
      Self::LinearGradient { rect, start, end } => {
        let span = (rect.2 - rect.0).abs().max(1.0);
        let t = ((x as f32 - rect.0) / span).clamp(0.0, 1.0);
        lerp_emf_plus_color(*start, *end, t)
      }
      Self::PathGradient {
        center,
        center_color,
        surround,
      } => {
        let distance = ((x as f32 - center.0).hypot(y as f32 - center.1) / 256.0).clamp(0.0, 1.0);
        lerp_emf_plus_color(*center_color, *surround, distance)
      }
      Self::Texture(image) => {
        if image.width == 0 || image.height == 0 {
          return EmfPlusRenderColor {
            color: EmfColor { r: 0, g: 0, b: 0 },
            alpha: u8::MAX,
          };
        }
        let tx = x.rem_euclid(image.width as i32) as usize;
        let ty = y.rem_euclid(image.height as i32) as usize;
        let offset = (ty * image.width + tx) * RGB_BYTES_PER_PIXEL;
        EmfPlusRenderColor {
          color: EmfColor {
            r: image.rgb[offset],
            g: image.rgb[offset + 1],
            b: image.rgb[offset + 2],
          },
          alpha: u8::MAX,
        }
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderFontKey {
  family: Option<String>,
  weight: u16,
  italic: bool,
}

#[derive(Clone, Debug)]
struct RenderFontFace {
  font_data: fontique::Blob<u8>,
  face_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RenderHintingKey {
  font: RenderFontKey,
  pixel_height_bits: u32,
  format: GdiGlyphFormat,
}

struct RenderFontCache {
  collection: FontCollection,
  source_cache: SourceCache,
  faces: HashMap<RenderFontKey, Option<RenderFontFace>>,
  hinting_instances: HashMap<RenderHintingKey, HintingInstance>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GdiVerticalDeviceMetrics {
  ascent: i32,
  descent: i32,
}

fn vdmx_vertical_device_metrics(
  table: &[u8],
  ppem: u16,
  char_set: u8,
) -> Option<GdiVerticalDeviceMetrics> {
  const ANSI_CHARSET: u8 = 0;
  const HEADER_SIZE: usize = 6;
  const RATIO_SIZE: usize = 4;
  const GROUP_HEADER_SIZE: usize = 4;
  const ENTRY_SIZE: usize = 6;

  let read_u16 = |offset: usize| {
    table
      .get(offset..offset + 2)
      .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
  };
  let version = read_u16(0)?;
  if version > 1 || read_u16(2)? == 0 {
    return None;
  }
  let ratio_count = usize::from(read_u16(4)?);
  let ratio_bytes = ratio_count.checked_mul(RATIO_SIZE)?;
  let offsets_start = HEADER_SIZE.checked_add(ratio_bytes)?;
  let offsets_end = offsets_start.checked_add(ratio_count.checked_mul(2)?)?;
  if offsets_end > table.len() {
    return None;
  }

  let mut group_offset = None;
  for index in 0..ratio_count {
    let ratio_offset = HEADER_SIZE + index * RATIO_SIZE;
    let ratio = table.get(ratio_offset..ratio_offset + RATIO_SIZE)?;
    let char_set_matches = match version {
      // Version 0 uses 1 for the Windows ANSI subset; 0 is the complete
      // symbol/dingbat repertoire. Microsoft specifies that Windows ignores
      // non-ANSI-subset entries for ANSI_CHARSET.
      0 => {
        (ratio[0] == 1 && char_set == ANSI_CHARSET) || (ratio[0] == 0 && char_set != ANSI_CHARSET)
      }
      // Version 1 uses 1 for the complete repertoire; 0 is additionally
      // available to ANSI_CHARSET consumers.
      1 => ratio[0] == 1 || (ratio[0] == 0 && char_set == ANSI_CHARSET),
      _ => false,
    };
    if !char_set_matches {
      continue;
    }
    let aspect_matches = (ratio[1] == 0 && ratio[2] == 0 && ratio[3] == 0)
      || (ratio[1] == 1 && ratio[2] <= 1 && ratio[3] >= 1);
    if aspect_matches {
      group_offset = Some(usize::from(read_u16(offsets_start + index * 2)?));
      break;
    }
  }

  let group_offset = group_offset?;
  let record_count = usize::from(read_u16(group_offset)?);
  let start_ppem = *table.get(group_offset + 2)?;
  let end_ppem = *table.get(group_offset + 3)?;
  if ppem < u16::from(start_ppem) || ppem > u16::from(end_ppem) {
    return None;
  }
  let entries_start = group_offset.checked_add(GROUP_HEADER_SIZE)?;
  let entries_end = entries_start.checked_add(record_count.checked_mul(ENTRY_SIZE)?)?;
  if entries_end > table.len() {
    return None;
  }
  for index in 0..record_count {
    let entry_offset = entries_start + index * ENTRY_SIZE;
    let entry_ppem = read_u16(entry_offset)?;
    if entry_ppem > ppem {
      break;
    }
    if entry_ppem == ppem {
      let y_max = i32::from(read_u16(entry_offset + 2)? as i16);
      let y_min = i32::from(read_u16(entry_offset + 4)? as i16);
      if y_max <= 0 || y_min > 0 {
        return None;
      }
      return Some(GdiVerticalDeviceMetrics {
        ascent: y_max,
        descent: y_min.saturating_abs(),
      });
    }
  }
  None
}

/// Destination class used by GDI when selecting a glyph bitmap format.
///
/// Wine's DIB driver forces `GGO_BITMAP` for destinations with at most eight
/// bits per pixel, while a color destination follows the realized LOGFONT
/// quality and system smoothing mode. A transparent metafile replay therefore
/// needs a color pass for source color and a separate monochrome pass for its
/// alpha plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GdiTextSurface {
  Color,
  Monochrome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GdiGlyphFormat {
  Monochrome,
  Grayscale,
  Lcd,
}

impl GdiTextSurface {
  fn glyph_format(self, quality: u8) -> GdiGlyphFormat {
    if self == Self::Monochrome {
      return GdiGlyphFormat::Monochrome;
    }
    match crate::wmf::WmfFontQuality::from_raw(quality) {
      Some(crate::wmf::WmfFontQuality::NonAntialiased) => GdiGlyphFormat::Monochrome,
      Some(crate::wmf::WmfFontQuality::Antialiased) => GdiGlyphFormat::Grayscale,
      // Wine only gives NONANTIALIASED_QUALITY and ANTIALIASED_QUALITY
      // unconditional formats. ClearType explicitly requests LCD and the
      // remaining quality values use the color device's smoothing default.
      _ => GdiGlyphFormat::Lcd,
    }
  }
}

#[derive(Clone, Debug)]
struct RenderedGlyph {
  left: i32,
  top: i32,
  width: usize,
  height: usize,
  mask: RenderedGlyphMask,
}

#[derive(Clone, Debug)]
enum RenderedGlyphMask {
  /// MSB-first one-bit rows padded to a DWORD boundary, matching GGO_BITMAP.
  Monochrome {
    stride: usize,
    bits: Vec<u8>,
  },
  Grayscale(Vec<u8>),
  Lcd(Vec<[u8; 3]>),
}

struct TextRenderRequest<'a> {
  font: &'a WmfTextFont,
  text: &'a str,
  x: f32,
  baseline_y: f32,
  height: f32,
  horizontal_scale: f32,
  advances: Option<&'a [f32]>,
  surface: GdiTextSurface,
}

impl RenderFontCache {
  fn load() -> Self {
    Self {
      collection: FontCollection::new(FontCollectionOptions {
        shared: false,
        system_fonts: true,
      }),
      source_cache: SourceCache::default(),
      faces: HashMap::new(),
      hinting_instances: HashMap::new(),
    }
  }

  fn resolve_face(&mut self, font: &WmfTextFont) -> Option<&RenderFontFace> {
    let key = RenderFontKey {
      family: font.family.clone(),
      weight: font.weight,
      italic: font.italic,
    };
    if !self.faces.contains_key(&key) {
      let mut families = Vec::with_capacity(2);
      if let Some(family) = key.family.as_deref() {
        families.push(QueryFamily::Named(family));
      }
      families.push(QueryFamily::Generic(GenericFamily::SansSerif));
      let weight = if key.weight == 0 {
        FontWeight::NORMAL
      } else {
        FontWeight::new(f32::from(key.weight.min(1000)))
      };
      let style = if key.italic {
        FontStyle::Italic
      } else {
        FontStyle::Normal
      };
      let mut query = self.collection.query(&mut self.source_cache);
      query.set_families(families);
      query.set_attributes(FontAttributes::new(FontWidth::NORMAL, style, weight));
      let mut face = None;
      query.matches_with(|font| {
        face = Some(RenderFontFace {
          font_data: font.blob.clone(),
          face_index: font.index,
        });
        QueryStatus::Stop
      });
      self.faces.insert(key.clone(), face);
    }
    self.faces.get(&key).and_then(Option::as_ref)
  }

  fn baseline_for_alignment(
    &mut self,
    font: &WmfTextFont,
    height: f32,
    reference_y: f32,
    alignment: WmfTextAlignmentModeFlags,
  ) -> f32 {
    if alignment.contains(WmfTextAlignmentModeFlags::BASELINE) {
      return reference_y;
    }
    let metrics = self.resolve_face(font).and_then(|face_data| {
      let face = FontRef::from_index(face_data.font_data.as_ref(), face_data.face_index).ok()?;
      // The OpenType VDMX table records the hinted yMax/yMin that Windows
      // uses for device Font Height. Wine's load_VDMX follows the negative
      // LOGFONT path by selecting the exact device ppem; positive lfHeight
      // uses a separate cell-height search and deliberately retains the
      // linear-metric fallback here.
      let device_metrics = (font.height < 0)
        .then(|| {
          let ppem = height.round().clamp(1.0, f32::from(u16::MAX)) as u16;
          face
            .table_data(FontTableTag::new(b"VDMX"))
            .and_then(|table| vdmx_vertical_device_metrics(table.as_bytes(), ppem, font.char_set))
        })
        .flatten();
      Some((
        face.metrics(FontSize::new(height.max(1.0)), LocationRef::default()),
        device_metrics,
      ))
    });
    if alignment.contains(WmfTextAlignmentModeFlags::BOTTOM) {
      reference_y
        + metrics.map_or(0.0, |(metrics, device_metrics)| {
          device_metrics.map_or_else(
            || gdi_realized_font_metric(metrics.descent),
            |metrics| metrics.descent as f32,
          )
        })
    } else {
      // [MS-WMF] 2.1.2.3 defines the all-zero vertical mode as TA_TOP.
      // Its reference point is the top of the font alignment box, so the
      // baseline is one font ascent below it. `lfHeight` is not the ascent:
      // substituting the character-cell height loses the hhea/OS/2 metrics
      // used by the GDI font mapper. NtGdiExtTextOutW consumes integer-device
      // TEXTMETRIC ascent/descent values after the world-to-device transform;
      // realize the scaled OpenType metric at that same boundary instead of
      // carrying a fractional baseline into glyph scan conversion.
      reference_y
        + metrics.map_or_else(
          || gdi_realized_font_metric(height),
          |(metrics, device_metrics)| {
            device_metrics.map_or_else(
              || gdi_realized_font_metric(metrics.ascent),
              |metrics| metrics.ascent as f32,
            )
          },
        )
    }
  }

  fn render_text(&mut self, request: &TextRenderRequest<'_>) -> Option<Vec<RenderedGlyph>> {
    let face_data = self.resolve_face(request.font)?.clone();
    let data = face_data.font_data.as_ref();
    let face = FontRef::from_index(data, face_data.face_index).ok()?;
    let size = FontSize::new(request.height.max(1.0));
    let location = LocationRef::new(&[]);
    let outlines = face.outline_glyphs();
    let charmap = face.charmap();
    let metrics = face.glyph_metrics(size, location);
    let format = request.surface.glyph_format(request.font.quality);
    let hinting_key = RenderHintingKey {
      font: RenderFontKey {
        family: request.font.family.clone(),
        weight: request.font.weight,
        italic: request.font.italic,
      },
      pixel_height_bits: request.height.max(1.0).to_bits(),
      format,
    };
    if !self.hinting_instances.contains_key(&hinting_key) {
      // Wine maps GGO_BITMAP to FT_LOAD_TARGET_MONO, GGO_GRAY* to
      // FT_LOAD_TARGET_NORMAL and horizontal subpixel output to
      // FT_LOAD_TARGET_LCD. Skrifa exposes those targets directly. Keep their
      // native interpreter settings: ExtTextOut's Dx array is applied to the
      // resulting glyph origins and does not require disabling horizontal
      // grid fitting inside each glyph.
      let target = match format {
        GdiGlyphFormat::Monochrome => Target::Mono,
        GdiGlyphFormat::Grayscale => SmoothMode::Normal.into(),
        GdiGlyphFormat::Lcd => SmoothMode::Lcd.into(),
      };
      let hinting = HintingInstance::new(
        &outlines,
        size,
        location,
        HintingOptions {
          engine: Default::default(),
          target,
        },
      )
      .ok()?;
      self.hinting_instances.insert(hinting_key.clone(), hinting);
    }
    let hinting = self.hinting_instances.get(&hinting_key)?;
    let mut cursor_x = request.x;
    let mut glyphs = Vec::with_capacity(request.text.chars().count());
    for (index, ch) in request.text.chars().enumerate() {
      if ch == '\n' || ch == '\r' {
        continue;
      }
      if ch.is_whitespace() {
        cursor_x += request
          .advances
          .and_then(|values| values.get(index))
          .copied()
          .unwrap_or(request.height * 0.35);
        continue;
      }
      let glyph_id = charmap.map(ch)?;
      let outline = outlines.get(glyph_id)?;
      let mut path_builder = TinySkiaPathBuilder::new();
      let mut collector = TinySkiaGlyphPathCollector {
        builder: &mut path_builder,
      };
      let adjusted_metrics = outline
        .draw(DrawSettings::hinted(hinting, false), &mut collector)
        .ok()?;
      if let Some(path) = path_builder.finish()
        && let Some(glyph) = rasterize_gdi_glyph(
          path,
          cursor_x,
          request.baseline_y,
          request.horizontal_scale,
          format,
        )
      {
        glyphs.push(glyph);
      }
      let advance = adjusted_metrics
        .advance_width
        .or_else(|| metrics.advance_width(glyph_id))
        .unwrap_or(request.height * 0.5);
      cursor_x += request
        .advances
        .and_then(|values| values.get(index))
        .copied()
        .unwrap_or(advance);
    }
    Some(glyphs)
  }
}

fn gdi_realized_font_metric(metric: f32) -> f32 {
  if metric.is_sign_negative() {
    -(-metric).round()
  } else {
    metric.round()
  }
}

/// Rasterizes one hinted outline using the bitmap formats selected by GDI.
///
/// Skrifa outlines use a Y-up baseline coordinate system. The device bitmap
/// is Y-down, so the path is reflected before its pixel bounds are rounded.
/// Monochrome output uses tiny-skia's integer scan converter and is then
/// packed exactly like `GGO_BITMAP`: MSB-first and DWORD-aligned per row.
fn rasterize_gdi_glyph(
  path: TinySkiaPath,
  cursor_x: f32,
  baseline_y: f32,
  horizontal_scale: f32,
  format: GdiGlyphFormat,
) -> Option<RenderedGlyph> {
  const CLEARTYPE_X_SCALE: i32 = 6;
  let x_samples = if format == GdiGlyphFormat::Lcd {
    CLEARTYPE_X_SCALE
  } else {
    1
  };
  let path = path.transform(TinySkiaTransform::from_row(
    horizontal_scale * x_samples as f32,
    0.0,
    0.0,
    -1.0,
    cursor_x.fract() * x_samples as f32,
    baseline_y.fract(),
  ))?;
  let bounds = path.compute_tight_bounds().unwrap_or_else(|| path.bounds());
  let local_left = bounds.left().floor() as i64;
  let local_top = bounds.top().floor() as i64;
  let local_right = bounds.right().ceil() as i64;
  let local_bottom = bounds.bottom().ceil() as i64;
  let width = u32::try_from(local_right.checked_sub(local_left)?).ok()?;
  let height = u32::try_from(local_bottom.checked_sub(local_top)?).ok()?;
  if width == 0 || height == 0 {
    return None;
  }
  let path = path.transform(TinySkiaTransform::from_translate(
    -(local_left as f32),
    -(local_top as f32),
  ))?;
  let mut mask = TinySkiaMask::new(width, height)?;
  mask.fill_path(
    &path,
    TinySkiaFillRule::Winding,
    format != GdiGlyphFormat::Monochrome,
    TinySkiaTransform::identity(),
  );
  let mut data = mask.take();
  if format == GdiGlyphFormat::Monochrome {
    apply_gdi_smart_dropout_control(&path, &mut data, width as usize, height as usize);
  }
  let top = (baseline_y.floor() as i32).saturating_add(i32::try_from(local_top).ok()?);

  match format {
    GdiGlyphFormat::Lcd => {
      let high_resolution_left = (cursor_x.floor() as i32)
        .saturating_mul(CLEARTYPE_X_SCALE)
        .saturating_add(i32::try_from(local_left).ok()?);
      let (left, width, coverage) =
        cleartype_box_decimate(&data, width as usize, height as usize, high_resolution_left);
      Some(RenderedGlyph {
        left,
        top,
        width,
        height: height as usize,
        mask: RenderedGlyphMask::Lcd(coverage),
      })
    }
    GdiGlyphFormat::Grayscale => Some(RenderedGlyph {
      left: (cursor_x.floor() as i32).saturating_add(i32::try_from(local_left).ok()?),
      top,
      width: width as usize,
      height: height as usize,
      mask: RenderedGlyphMask::Grayscale(data),
    }),
    GdiGlyphFormat::Monochrome => {
      let (stride, bits) = pack_gdi_monochrome_mask(&data, width as usize, height as usize)?;
      Some(RenderedGlyph {
        left: (cursor_x.floor() as i32).saturating_add(i32::try_from(local_left).ok()?),
        top,
        width: width as usize,
        height: height as usize,
        mask: RenderedGlyphMask::Monochrome { stride, bits },
      })
    }
  }
}

/// A line segment used by the local monochrome drop-out scanner.
///
/// Skrifa has already grid-fitted the outline.  The remaining operation is
/// scan conversion, so these coordinates are in final device pixels.
#[derive(Clone, Copy, Debug)]
struct GdiDropoutLine {
  start: GdiDropoutPoint,
  end: GdiDropoutPoint,
}

#[derive(Clone, Copy, Debug)]
struct GdiDropoutPoint {
  x: f32,
  y: f32,
}

impl From<TinySkiaPoint> for GdiDropoutPoint {
  fn from(point: TinySkiaPoint) -> Self {
    Self {
      x: point.x,
      y: point.y,
    }
  }
}

#[derive(Clone, Copy, Debug)]
struct GdiDropoutSpan {
  start: f32,
  end: f32,
}

#[derive(Clone, Copy, Debug)]
enum GdiDropoutAxis {
  Horizontal,
  Vertical,
}

/// Adds the pixels required by the OpenType smart drop-out rules to a
/// tiny-skia bilevel mask.
///
/// The public Skrifa API returns the hinted outline but not the final
/// `SCANCTRL`/`SCANTYPE` graphics state.  This deliberately bounded fallback
/// therefore uses one deterministic mode: smart drop-outs excluding stubs
/// (OpenType scan-conversion rules 5 and 6).  Like FreeType's monochrome
/// rasterizer, it scans in both directions and never changes antialiased text.
fn apply_gdi_smart_dropout_control(
  path: &TinySkiaPath,
  coverage: &mut [u8],
  width: usize,
  height: usize,
) {
  if coverage.len() != width.saturating_mul(height) || width == 0 || height == 0 {
    return;
  }

  let lines = flatten_gdi_dropout_path(path);
  if lines.is_empty() {
    return;
  }

  let horizontal_spans = collect_gdi_dropout_spans(&lines, height, GdiDropoutAxis::Horizontal);
  apply_gdi_dropout_spans(
    &horizontal_spans,
    coverage,
    width,
    height,
    GdiDropoutAxis::Horizontal,
  );

  let vertical_spans = collect_gdi_dropout_spans(&lines, width, GdiDropoutAxis::Vertical);
  apply_gdi_dropout_spans(
    &vertical_spans,
    coverage,
    width,
    height,
    GdiDropoutAxis::Vertical,
  );
}

fn flatten_gdi_dropout_path(path: &TinySkiaPath) -> Vec<GdiDropoutLine> {
  let mut lines = Vec::new();
  let mut current = None;
  let mut segments = path.segments();
  segments.set_auto_close(true);

  for segment in segments {
    match segment {
      TinySkiaPathSegment::MoveTo(point) => current = Some(point.into()),
      TinySkiaPathSegment::LineTo(point) => {
        let point = GdiDropoutPoint::from(point);
        if let Some(start) = current {
          push_gdi_dropout_line(&mut lines, start, point);
        }
        current = Some(point);
      }
      TinySkiaPathSegment::QuadTo(control, end) => {
        let control = GdiDropoutPoint::from(control);
        let end = GdiDropoutPoint::from(end);
        if let Some(start) = current {
          flatten_gdi_dropout_quad(start, control, end, 0, &mut lines);
        }
        current = Some(end);
      }
      TinySkiaPathSegment::CubicTo(control1, control2, end) => {
        let control1 = GdiDropoutPoint::from(control1);
        let control2 = GdiDropoutPoint::from(control2);
        let end = GdiDropoutPoint::from(end);
        if let Some(start) = current {
          flatten_gdi_dropout_cubic(start, control1, control2, end, 0, &mut lines);
        }
        current = Some(end);
      }
      TinySkiaPathSegment::Close => current = None,
    }
  }

  lines
}

fn push_gdi_dropout_line(
  lines: &mut Vec<GdiDropoutLine>,
  start: GdiDropoutPoint,
  end: GdiDropoutPoint,
) {
  if start.x != end.x || start.y != end.y {
    lines.push(GdiDropoutLine { start, end });
  }
}

fn midpoint_gdi_dropout_point(first: GdiDropoutPoint, second: GdiDropoutPoint) -> GdiDropoutPoint {
  GdiDropoutPoint {
    x: (first.x + second.x) * 0.5,
    y: (first.y + second.y) * 0.5,
  }
}

fn squared_gdi_dropout_distance_to_line(
  point: GdiDropoutPoint,
  start: GdiDropoutPoint,
  end: GdiDropoutPoint,
) -> f32 {
  let dx = end.x - start.x;
  let dy = end.y - start.y;
  let length_squared = dx * dx + dy * dy;
  if length_squared == 0.0 {
    let px = point.x - start.x;
    let py = point.y - start.y;
    return px * px + py * py;
  }

  let cross = dx * (point.y - start.y) - dy * (point.x - start.x);
  cross * cross / length_squared
}

fn flatten_gdi_dropout_quad(
  start: GdiDropoutPoint,
  control: GdiDropoutPoint,
  end: GdiDropoutPoint,
  depth: u8,
  lines: &mut Vec<GdiDropoutLine>,
) {
  // FreeType's normal B/W raster precision is 1/64 pixel.  Matching that
  // granularity is sufficient for deciding whether a pixel-center gap was
  // crossed, while the depth guard bounds malformed or extreme curves.
  const TOLERANCE_SQUARED: f32 = 1.0 / (64.0 * 64.0);
  const MAX_DEPTH: u8 = 12;
  if depth == MAX_DEPTH
    || squared_gdi_dropout_distance_to_line(control, start, end) <= TOLERANCE_SQUARED
  {
    push_gdi_dropout_line(lines, start, end);
    return;
  }

  let start_control = midpoint_gdi_dropout_point(start, control);
  let control_end = midpoint_gdi_dropout_point(control, end);
  let midpoint = midpoint_gdi_dropout_point(start_control, control_end);
  flatten_gdi_dropout_quad(start, start_control, midpoint, depth + 1, lines);
  flatten_gdi_dropout_quad(midpoint, control_end, end, depth + 1, lines);
}

fn flatten_gdi_dropout_cubic(
  start: GdiDropoutPoint,
  control1: GdiDropoutPoint,
  control2: GdiDropoutPoint,
  end: GdiDropoutPoint,
  depth: u8,
  lines: &mut Vec<GdiDropoutLine>,
) {
  const TOLERANCE_SQUARED: f32 = 1.0 / (64.0 * 64.0);
  const MAX_DEPTH: u8 = 12;
  let flatness = squared_gdi_dropout_distance_to_line(control1, start, end)
    .max(squared_gdi_dropout_distance_to_line(control2, start, end));
  if depth == MAX_DEPTH || flatness <= TOLERANCE_SQUARED {
    push_gdi_dropout_line(lines, start, end);
    return;
  }

  let start_control = midpoint_gdi_dropout_point(start, control1);
  let controls = midpoint_gdi_dropout_point(control1, control2);
  let control_end = midpoint_gdi_dropout_point(control2, end);
  let left_control = midpoint_gdi_dropout_point(start_control, controls);
  let right_control = midpoint_gdi_dropout_point(controls, control_end);
  let midpoint = midpoint_gdi_dropout_point(left_control, right_control);
  flatten_gdi_dropout_cubic(
    start,
    start_control,
    left_control,
    midpoint,
    depth + 1,
    lines,
  );
  flatten_gdi_dropout_cubic(midpoint, right_control, control_end, end, depth + 1, lines);
}

fn collect_gdi_dropout_spans(
  lines: &[GdiDropoutLine],
  scan_count: usize,
  axis: GdiDropoutAxis,
) -> Vec<Vec<GdiDropoutSpan>> {
  let mut scans = Vec::with_capacity(scan_count);
  for scan_index in 0..scan_count {
    let scan_coordinate = scan_index as f32 + 0.5;
    let mut crossings = Vec::new();
    for line in lines {
      let (start_axis, end_axis, start_cross, end_cross) = match axis {
        GdiDropoutAxis::Horizontal => (line.start.y, line.end.y, line.start.x, line.end.x),
        GdiDropoutAxis::Vertical => (line.start.x, line.end.x, line.start.y, line.end.y),
      };
      let crossing = if start_axis <= scan_coordinate && scan_coordinate < end_axis {
        Some(1)
      } else if end_axis <= scan_coordinate && scan_coordinate < start_axis {
        Some(-1)
      } else {
        None
      };
      let Some(winding) = crossing else {
        continue;
      };
      let t = (scan_coordinate - start_axis) / (end_axis - start_axis);
      crossings.push((start_cross + (end_cross - start_cross) * t, winding));
    }
    crossings.sort_by(|left, right| left.0.total_cmp(&right.0));

    let mut winding = 0i32;
    let mut span_start = None;
    let mut spans = Vec::new();
    for (coordinate, delta) in crossings {
      let previous_winding = winding;
      winding += delta;
      if previous_winding == 0 && winding != 0 {
        span_start = Some(coordinate);
      } else if previous_winding != 0
        && winding == 0
        && let Some(start) = span_start.take()
        && coordinate > start
      {
        spans.push(GdiDropoutSpan {
          start,
          end: coordinate,
        });
      }
    }
    scans.push(spans);
  }
  scans
}

fn apply_gdi_dropout_spans(
  scans: &[Vec<GdiDropoutSpan>],
  coverage: &mut [u8],
  width: usize,
  height: usize,
  axis: GdiDropoutAxis,
) {
  if scans.len() < 3 {
    return;
  }

  for scan_index in 1..scans.len() - 1 {
    for span in &scans[scan_index] {
      let first_center = (span.start - 0.5).ceil() as i32;
      let last_center = (span.end - 0.5).floor() as i32;
      if first_center <= last_center {
        continue;
      }

      // Rule 6 excludes a stub unless both contours continue to intersect
      // scan lines in both directions.  Interval overlap is the local
      // equivalent for the flattened hinted outline; steep features are
      // recovered by the perpendicular pass.
      if !gdi_dropout_span_continues(*span, &scans[scan_index - 1])
        || !gdi_dropout_span_continues(*span, &scans[scan_index + 1])
      {
        continue;
      }

      let lower = (span.start - 0.5).floor() as i32;
      let Some(upper) = lower.checked_add(1) else {
        continue;
      };
      let midpoint = (span.start + span.end) * 0.5;
      // FreeType's SMART macro has a 63/64-pixel pre-division bias.  In
      // device coordinates this is a 1/128-pixel tie bias toward the left;
      // reflection into the Y-down bitmap reverses it for vertical scans.
      const SMART_TIE_BIAS: f32 = 1.0 / 128.0;
      let preferred = match axis {
        GdiDropoutAxis::Horizontal => (midpoint - SMART_TIE_BIAS).floor() as i32,
        GdiDropoutAxis::Vertical => (midpoint + SMART_TIE_BIAS).floor() as i32,
      }
      .clamp(lower, upper);
      let other = if preferred == lower { upper } else { lower };

      let preferred_index = gdi_dropout_mask_index(axis, scan_index, preferred, width, height);
      let other_index = gdi_dropout_mask_index(axis, scan_index, other, width, height);
      if let Some(preferred_index) = preferred_index {
        if other_index.is_some_and(|index| coverage[index] != 0) {
          continue;
        }
        coverage[preferred_index] = u8::MAX;
      } else if let Some(other_index) = other_index {
        // FreeType keeps the drop-out inside the glyph bitmap when its
        // preferred pixel lies just outside the rounded bounding box.
        coverage[other_index] = u8::MAX;
      }
    }
  }
}

fn gdi_dropout_span_continues(span: GdiDropoutSpan, adjacent: &[GdiDropoutSpan]) -> bool {
  adjacent
    .iter()
    .any(|candidate| candidate.start < span.end && span.start < candidate.end)
}

fn gdi_dropout_mask_index(
  axis: GdiDropoutAxis,
  scan_index: usize,
  pixel: i32,
  width: usize,
  height: usize,
) -> Option<usize> {
  let pixel = usize::try_from(pixel).ok()?;
  match axis {
    GdiDropoutAxis::Horizontal if scan_index < height && pixel < width => {
      scan_index.checked_mul(width)?.checked_add(pixel)
    }
    GdiDropoutAxis::Vertical if scan_index < width && pixel < height => {
      pixel.checked_mul(width)?.checked_add(scan_index)
    }
    _ => None,
  }
}

fn pack_gdi_monochrome_mask(
  coverage: &[u8],
  width: usize,
  height: usize,
) -> Option<(usize, Vec<u8>)> {
  if coverage.len() != width.checked_mul(height)? {
    return None;
  }
  let stride = width.checked_add(31)?.checked_div(32)?.checked_mul(4)?;
  let mut bits = vec![0; stride.checked_mul(height)?];
  for y in 0..height {
    for x in 0..width {
      if coverage[y * width + x] != 0 {
        bits[y * stride + x / 8] |= 0x80 >> (x % 8);
      }
    }
  }
  Some((stride, bits))
}

fn gdi_subpixel_blend(destination: u8, text: u8, alpha: u8) -> u8 {
  ((u32::from(text) * u32::from(alpha) + u32::from(destination) * u32::from(u8::MAX - alpha) + 127)
    / 255) as u8
}

/// Seventeen-level grayscale intensity ramp used by the Wine DIB GDI driver.
///
/// `GGO_GRAY4_BITMAP` exposes coverage levels 0..=16. Windows' grayscale text
/// blend is not the same operation as ordinary source alpha; the ramp and the
/// destination-relative interpolation below are Wine's source-backed model of
/// native GDI output.
fn gdi_grayscale_blend(destination: EmfColor, text: EmfColor, level: usize) -> EmfColor {
  const RAMP: [u8; 17] = [
    0x00, 0x4D, 0x68, 0x7C, 0x8C, 0x9A, 0xA7, 0xB2, 0xBD, 0xC7, 0xD0, 0xD9, 0xE1, 0xE9, 0xF0, 0xF8,
    0xFF,
  ];
  let blend = |destination: u8, text: u8| {
    let minimum = u16::from(RAMP[level]) * u16::from(text) / 255;
    let reverse = u16::from(RAMP[16 - level]);
    let maximum = reverse + (255 - reverse) * u16::from(text) / 255;
    if destination == text {
      destination
    } else if destination > text {
      let difference = u32::from(destination - text);
      let range = u32::from(maximum as u8 - text);
      text + ((difference * range) / u32::from(255 - text)) as u8
    } else {
      let difference = u32::from(text - destination);
      let range = u32::from(text - minimum as u8);
      text - ((difference * range) / u32::from(text)) as u8
    }
  };
  EmfColor {
    r: blend(destination.r, text.r),
    g: blend(destination.g, text.g),
    b: blend(destination.b, text.b),
  }
}

/// Applies the one-pixel-wide displaced box filters described by Microsoft's
/// ClearType RGB-decimation paper to a six-times-horizontal alpha raster.
fn cleartype_box_decimate(
  high_resolution: &[u8],
  high_resolution_width: usize,
  height: usize,
  high_resolution_left: i32,
) -> (i32, usize, Vec<[u8; 3]>) {
  const SAMPLES_PER_PIXEL: i32 = 6;
  if high_resolution_width == 0 || height == 0 {
    return (0, 0, Vec::new());
  }
  let high_resolution_right = high_resolution_left.saturating_add(high_resolution_width as i32);
  let left = high_resolution_left.div_euclid(SAMPLES_PER_PIXEL) - 1;
  let right = (high_resolution_right - 1).div_euclid(SAMPLES_PER_PIXEL) + 2;
  let width = (right - left).max(0) as usize;
  let mut output = vec![[0; 3]; width * height];

  for y in 0..height {
    let row = &high_resolution[y * high_resolution_width..(y + 1) * high_resolution_width];
    for output_x in left..right {
      let mut channels = [0_u8; 3];
      for (channel, window_offset) in [-2_i32, 0, 2].into_iter().enumerate() {
        let window_start = output_x
          .saturating_mul(SAMPLES_PER_PIXEL)
          .saturating_add(window_offset);
        let mut sum = 0_u16;
        for sample_x in window_start..window_start + SAMPLES_PER_PIXEL {
          let source_x = sample_x - high_resolution_left;
          if let Ok(source_x) = usize::try_from(source_x)
            && let Some(sample) = row.get(source_x)
          {
            sum += u16::from(*sample);
          }
        }
        channels[channel] = ((sum + 3) / SAMPLES_PER_PIXEL as u16) as u8;
      }
      output[y * width + (output_x - left) as usize] = channels;
    }
  }
  (left, width, output)
}

struct TinySkiaGlyphPathCollector<'a> {
  builder: &'a mut TinySkiaPathBuilder,
}

impl OutlinePen for TinySkiaGlyphPathCollector<'_> {
  fn move_to(&mut self, x: f32, y: f32) {
    self.builder.move_to(x, y);
  }

  fn line_to(&mut self, x: f32, y: f32) {
    self.builder.line_to(x, y);
  }

  fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
    self.builder.quad_to(x1, y1, x, y);
  }

  fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
    self.builder.cubic_to(x1, y1, x2, y2, x, y);
  }

  fn close(&mut self) {
    self.builder.close();
  }
}

impl EmfVectorState {
  fn new_with_options(data: &[u8], options: RenderOptions) -> Result<Self, String> {
    Self::new_with_options_and_text_surface(data, options, GdiTextSurface::Color)
  }

  fn new_with_options_and_text_surface(
    data: &[u8],
    options: RenderOptions,
    text_surface: GdiTextSurface,
  ) -> Result<Self, String> {
    let geometry = emf_playback_geometry(data)?;
    let natural_width = geometry.width;
    let natural_height = geometry.height;
    let (width, height) = options.resolved_canvas_size(natural_width, natural_height);
    let output_scale_x = width as f32 / natural_width.max(1) as f32;
    let output_scale_y = height as f32 / natural_height.max(1) as f32;
    let background_color = options.background_color.unwrap_or([255; 3]);
    let mut rgb = vec![0; width * height * RGB_BYTES_PER_PIXEL];
    for pixel in rgb.chunks_exact_mut(RGB_BYTES_PER_PIXEL) {
      pixel.copy_from_slice(&background_color);
    }

    Ok(Self {
      width,
      height,
      natural_width,
      natural_height,
      playback_origin_x: geometry.origin_x,
      playback_origin_y: geometry.origin_y,
      playback_scale_x: geometry.scale_x,
      playback_scale_y: geometry.scale_y,
      output_scale_x,
      output_scale_y,
      map_mode: EmrMapMode::Text,
      window_org_x: 0,
      window_org_y: 0,
      window_ext_x: natural_width as i32,
      window_ext_y: natural_height as i32,
      viewport_org_x: 0,
      viewport_org_y: 0,
      viewport_ext_x: natural_width as i32,
      viewport_ext_y: natural_height as i32,
      world_transform: EmfTransform::identity(),
      // Classic EMF records are already expressed in the reference device's
      // logical/device coordinate pipeline. EMF+ Header switches this to the
      // GDI+ default UnitDisplay state before any EMF+ drawing record runs.
      emf_plus_page_unit: EmfPlusUnitType::Pixel,
      emf_plus_page_scale: 1.0,
      emf_plus_logical_dpi_x: 96.0,
      emf_plus_logical_dpi_y: 96.0,
      emf_plus_video_display: true,
      brush_colors: std::collections::HashMap::new(),
      solid_brushes: std::collections::HashSet::new(),
      pens: std::collections::HashMap::new(),
      fonts: std::collections::HashMap::new(),
      current_brush: None,
      current_brush_is_solid: false,
      current_pen: Some(EmfPen {
        color: EmfColor { r: 0, g: 0, b: 0 },
        alpha: u8::MAX,
        width: 1,
        width_space: EmfPenWidthSpace::Device,
      }),
      current_font: None,
      current_pos: EmfPoint { x: 0, y: 0 },
      text_color: EmfColor { r: 0, g: 0, b: 0 },
      binary_raster_operation: WmfBinaryRasterOperation::CopyPen,
      text_alignment: WmfTextAlignmentModeFlags::empty(),
      clip_rect: None,
      clip_mask: None,
      saved_states: Vec::new(),
      emf_plus_saved_states: Vec::new(),
      emf_plus_containers: Vec::new(),
      emf_plus_objects: Vec::new(),
      emf_plus_object_assembler: EmfPlusObjectAssembler::default(),
      font_cache: RenderFontCache::load(),
      text_surface,
      suppress_text: options.suppress_text,
      rgb,
    })
  }

  fn map_point(&self, point: EmfPoint) -> (f32, f32) {
    let (x, y) = self.world_transform.apply(point);
    let (page_scale_x, page_scale_y) = self.emf_plus_page_device_scale();
    let x = x * page_scale_x;
    let y = y * page_scale_y;
    let (scale_x, scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    (
      (self.viewport_org_x as f32 + (x - self.window_org_x as f32) * scale_x
        - self.playback_origin_x)
        * self.playback_scale_x
        * self.output_scale_x,
      (self.viewport_org_y as f32 + (y - self.window_org_y as f32) * scale_y
        - self.playback_origin_y)
        * self.playback_scale_y
        * self.output_scale_y,
    )
  }

  fn emf_plus_page_device_scale(&self) -> (f32, f32) {
    (
      emf_plus_units_to_device_scale(
        self.emf_plus_page_unit,
        self.emf_plus_page_scale,
        self.emf_plus_logical_dpi_x,
        self.emf_plus_video_display,
      ),
      emf_plus_units_to_device_scale(
        self.emf_plus_page_unit,
        self.emf_plus_page_scale,
        self.emf_plus_logical_dpi_y,
        self.emf_plus_video_display,
      ),
    )
  }

  fn resolve_pen(&self, mut pen: EmfPen) -> EmfPen {
    if pen.width_space == EmfPenWidthSpace::Device {
      return pen;
    }
    let width = pen.width as f32;
    let (page_scale_x, page_scale_y) = self.emf_plus_page_device_scale();
    let (extent_scale_x, extent_scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    let scale_x = extent_scale_x * page_scale_x * self.playback_scale_x * self.output_scale_x;
    let scale_y = extent_scale_y * page_scale_y * self.playback_scale_y * self.output_scale_y;
    let x_axis = (
      width * self.world_transform.m11 * scale_x,
      width * self.world_transform.m12 * scale_y,
    );
    let y_axis = (
      width * self.world_transform.m21 * scale_x,
      width * self.world_transform.m22 * scale_y,
    );
    let width = match pen.width_space {
      EmfPenWidthSpace::Device => unreachable!("device pen returned before width mapping"),
      EmfPenWidthSpace::LogicalX => x_axis.0.hypot(x_axis.1),
      EmfPenWidthSpace::World => x_axis.0.hypot(x_axis.1).max(y_axis.0.hypot(y_axis.1)),
    };
    pen.width = if width.is_finite() {
      width.round().max(1.0) as usize
    } else {
      1
    };
    pen.width_space = EmfPenWidthSpace::Device;
    pen
  }

  fn set_pixel(&mut self, x: i32, y: i32, color: EmfColor) {
    if x < 0 || y < 0 {
      return;
    }
    if let Some((left, top, right, bottom)) = self.clip_rect
      && (x < left || x >= right || y < top || y >= bottom)
    {
      return;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= self.width || y >= self.height {
      return;
    }
    if let Some(mask) = &self.clip_mask
      && !mask[y * self.width + x]
    {
      return;
    }
    let offset = (y * self.width + x) * RGB_BYTES_PER_PIXEL;
    self.rgb[offset] = color.r;
    self.rgb[offset + 1] = color.g;
    self.rgb[offset + 2] = color.b;
  }

  fn set_pixel_with_alpha(&mut self, x: i32, y: i32, color: EmfColor, alpha: u8) {
    if alpha == u8::MAX {
      self.set_pixel(x, y, color);
      return;
    }
    if alpha == 0 {
      return;
    }
    let Some(destination) = self.pixel(x, y) else {
      return;
    };
    let blend = |source: u8, destination: u8| {
      let alpha = u32::from(alpha);
      ((u32::from(source) * alpha + u32::from(destination) * (255 - alpha) + 127) / 255) as u8
    };
    self.set_pixel(
      x,
      y,
      EmfColor {
        r: blend(color.r, destination.r),
        g: blend(color.g, destination.g),
        b: blend(color.b, destination.b),
      },
    );
  }

  fn set_vector_pixel(&mut self, x: i32, y: i32, color: EmfColor) {
    self.set_vector_pixel_with_alpha(x, y, color, u8::MAX);
  }

  fn set_vector_pixel_with_alpha(&mut self, x: i32, y: i32, color: EmfColor, alpha: u8) {
    let Some(destination) = self.pixel(x, y) else {
      return;
    };
    self.set_pixel_with_alpha(
      x,
      y,
      apply_binary_raster_operation(color, destination, self.binary_raster_operation),
      alpha,
    );
  }

  fn pixel(&self, x: i32, y: i32) -> Option<EmfColor> {
    let (x, y) = (usize::try_from(x).ok()?, usize::try_from(y).ok()?);
    if x >= self.width || y >= self.height {
      return None;
    }
    let offset = (y * self.width + x) * RGB_BYTES_PER_PIXEL;
    Some(EmfColor {
      r: self.rgb[offset],
      g: self.rgb[offset + 1],
      b: self.rgb[offset + 2],
    })
  }

  fn draw_rgb_image(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &RasterPixels,
  ) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    // StretchBlt's destination extent is half-open. Office/GDI maps the
    // leading edge to the nearest device pixel and truncates the exclusive
    // trailing edge; rounding both edges makes a half-pixel bottom grow by
    // one row (as in the 32-unit preview bitmap in tdf135653.docx).
    let right = mapped_left.max(mapped_right).floor() as i32;
    let bottom = mapped_top.max(mapped_bottom).floor() as i32;
    let width = (right - left).max(1);
    let height = (bottom - top).max(1);
    let interpolate = width as usize != image.width || height as usize != image.height;

    for y in 0..height {
      for x in 0..width {
        let color = if interpolate {
          bilinear_raster_color(
            image,
            x as usize,
            y as usize,
            width as usize,
            height as usize,
          )
        } else {
          raster_color(image, x as usize, y as usize)
        };
        self.set_pixel(left + x, top + y, color);
      }
    }
  }

  fn draw_alpha_blended_image(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &AlphaBlendRaster,
    source_constant_alpha: u8,
  ) {
    if source_constant_alpha == 0 {
      return;
    }
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    let right = mapped_left.max(mapped_right).floor() as i32;
    let bottom = mapped_top.max(mapped_bottom).floor() as i32;
    let width = (right - left).max(1);
    let height = (bottom - top).max(1);
    let interpolate =
      width as usize != image.pixels.width || height as usize != image.pixels.height;

    for y in 0..height {
      for x in 0..width {
        let (color, source_alpha) = if interpolate {
          (
            bilinear_raster_color(
              &image.pixels,
              x as usize,
              y as usize,
              width as usize,
              height as usize,
            ),
            image.source_alpha.as_deref().map(|alpha| {
              bilinear_raster_plane_value(
                alpha,
                image.pixels.width,
                image.pixels.height,
                x as usize,
                y as usize,
                width as usize,
                height as usize,
              )
            }),
          )
        } else {
          let source_x = x as usize;
          let source_y = y as usize;
          (
            raster_color(&image.pixels, source_x, source_y),
            image
              .source_alpha
              .as_ref()
              .map(|alpha| alpha[source_y * image.pixels.width + source_x]),
          )
        };
        let Some(destination) = self.pixel(left + x, top + y) else {
          continue;
        };
        self.set_pixel(
          left + x,
          top + y,
          gdi_alpha_blend_color(destination, color, source_alpha, source_constant_alpha),
        );
      }
    }
  }

  fn draw_rgb_image_with_rop(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &RasterPixels,
    rop: WmfTernaryRasterOperationCode,
  ) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    let right = mapped_left.max(mapped_right).floor() as i32;
    let bottom = mapped_top.max(mapped_bottom).floor() as i32;
    let width = (right - left).max(1);
    let height = (bottom - top).max(1);
    // A one-bit mask in the canonical SRCAND/SRCINVERT transparency pair must
    // keep its boolean samples. Color sources follow the filtered StretchBlt
    // path used by Office/GDI+ when the destination viewport changes size.
    let interpolate = (width as usize != image.width || height as usize != image.height)
      && !is_discrete_two_color_raster(image);

    for y in 0..height {
      for x in 0..width {
        let dest_x = left + x;
        let dest_y = top + y;
        let src = if interpolate {
          bilinear_raster_color(
            image,
            x as usize,
            y as usize,
            width as usize,
            height as usize,
          )
        } else {
          let src_x = nearest_raster_index(x as usize, width as usize, image.width);
          let src_y = nearest_raster_index(y as usize, height as usize, image.height);
          raster_color(image, src_x, src_y)
        };
        if let Some(color) = self.apply_raster_op(dest_x, dest_y, src, rop) {
          self.set_pixel(dest_x, dest_y, color);
        }
      }
    }
  }

  fn draw_masked_rgb_image(
    &mut self,
    dest_x: i32,
    dest_y: i32,
    dest_width: i32,
    dest_height: i32,
    image: &RasterPixels,
    mask: &RasterPixels,
  ) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint {
      x: dest_x,
      y: dest_y,
    });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: dest_x + dest_width,
      y: dest_y + dest_height,
    });
    let left = mapped_left.min(mapped_right).round() as i32;
    let top = mapped_top.min(mapped_bottom).round() as i32;
    let right = mapped_left.max(mapped_right).floor() as i32;
    let bottom = mapped_top.max(mapped_bottom).floor() as i32;
    let width = (right - left).max(1) as usize;
    let height = (bottom - top).max(1) as usize;
    let interpolate = width != image.width || height != image.height;

    for y in 0..height {
      let mask_y = nearest_raster_index(y, height, mask.height);
      for x in 0..width {
        let mask_x = nearest_raster_index(x, width, mask.width);
        let mask_color = raster_color(mask, mask_x, mask_y);
        let color = if interpolate {
          gdi_plus_bilinear_raster_color(image, x, y, width, height)
        } else {
          raster_color(image, x, y)
        };
        // The canonical SRCAND mask uses black for covered source pixels and
        // white for the transparent destination. GDI+ filters the paired
        // SRCINVERT color bitmap independently, and a nonblack filtered
        // sample is consequently part of the pair's opaque output even when
        // its nearest one-bit mask sample is white. Keeping that color fringe
        // is required before black/white destination reconstruction; masking
        // first contracts icon edges.
        if u16::from(mask_color.r) + u16::from(mask_color.g) + u16::from(mask_color.b) >= 3 * 128
          && color == (EmfColor { r: 0, g: 0, b: 0 })
        {
          continue;
        }
        self.set_pixel(left + x as i32, top + y as i32, color);
      }
    }
  }

  fn fill_rect_with_rop(
    &mut self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    rop: WmfTernaryRasterOperationCode,
  ) {
    let brush = match self.current_brush {
      Some(brush) => brush,
      None if rop.uses_pattern() || rop.uses_source() => return,
      None => EmfColor { r: 0, g: 0, b: 0 },
    };
    let (mapped_left, mapped_top) = self.map_point(EmfPoint { x: left, y: top });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let left = mapped_left.min(mapped_right).round().max(0.0) as i32;
    let top = mapped_top.min(mapped_bottom).round().max(0.0) as i32;
    let right = mapped_left.max(mapped_right).round().min(self.width as f32) as i32;
    let bottom = mapped_top
      .max(mapped_bottom)
      .round()
      .min(self.height as f32) as i32;
    for y in top..bottom {
      for x in left..right {
        if let Some(color) = self.apply_raster_op(x, y, brush, rop) {
          self.set_pixel(x, y, color);
        }
      }
    }
  }

  fn apply_raster_op(
    &self,
    x: i32,
    y: i32,
    src: EmfColor,
    rop: WmfTernaryRasterOperationCode,
  ) -> Option<EmfColor> {
    self.apply_raster_op_with_pattern(x, y, src, self.current_brush.unwrap_or(src), rop)
  }

  fn apply_raster_op_with_pattern(
    &self,
    x: i32,
    y: i32,
    src: EmfColor,
    pattern: EmfColor,
    rop: WmfTernaryRasterOperationCode,
  ) -> Option<EmfColor> {
    let dest = self.pixel_color(x, y).unwrap_or(EmfColor {
      r: 255,
      g: 255,
      b: 255,
    });
    let color = match rop {
      WmfTernaryRasterOperationCode::BLACKNESS => EmfColor { r: 0, g: 0, b: 0 },
      WmfTernaryRasterOperationCode::WHITENESS => EmfColor {
        r: 255,
        g: 255,
        b: 255,
      },
      WmfTernaryRasterOperationCode::DSTINVERT => dest.not(),
      WmfTernaryRasterOperationCode::NOTSRCCOPY => src.not(),
      WmfTernaryRasterOperationCode::SRCCOPY => src,
      WmfTernaryRasterOperationCode::SRCPAINT => src.or(dest),
      WmfTernaryRasterOperationCode::SRCAND => src.and(dest),
      WmfTernaryRasterOperationCode::SRCINVERT => src.xor(dest),
      WmfTernaryRasterOperationCode::SRCERASE => src.and(dest.not()),
      WmfTernaryRasterOperationCode::MERGECOPY => src.and(pattern),
      WmfTernaryRasterOperationCode::MERGEPAINT => src.not().or(dest),
      WmfTernaryRasterOperationCode::PATCOPY => pattern,
      WmfTernaryRasterOperationCode::PATINVERT => pattern.xor(dest),
      WmfTernaryRasterOperationCode::PATPAINT => pattern.or(src.not()).or(dest),
      WmfTernaryRasterOperationCode::D => return None,
      _ => return None,
    };
    Some(color)
  }

  fn pixel_color(&self, x: i32, y: i32) -> Option<EmfColor> {
    if x < 0 || y < 0 {
      return None;
    }
    let (x, y) = (x as usize, y as usize);
    if x >= self.width || y >= self.height {
      return None;
    }
    let offset = (y * self.width + x) * RGB_BYTES_PER_PIXEL;
    Some(EmfColor {
      r: self.rgb[offset],
      g: self.rgb[offset + 1],
      b: self.rgb[offset + 2],
    })
  }

  fn mapped_vertical_length(&self, logical_height: i32) -> f32 {
    let height = logical_height.unsigned_abs().max(1) as f32;
    let (x, y) = self.map_vector(0.0, height);
    x.hypot(y).max(1.0)
  }

  fn mapped_horizontal_length(&self, logical_width: i32) -> f32 {
    let width = logical_width.unsigned_abs().max(1) as f32;
    let (x, y) = self.map_vector(width, 0.0);
    x.hypot(y).max(f32::EPSILON)
  }

  fn map_vector(&self, x: f32, y: f32) -> (f32, f32) {
    let mapped_x = x * self.world_transform.m11 + y * self.world_transform.m21;
    let mapped_y = x * self.world_transform.m12 + y * self.world_transform.m22;
    let (page_scale_x, page_scale_y) = self.emf_plus_page_device_scale();
    let (extent_scale_x, extent_scale_y) = emf_window_viewport_scale(
      self.map_mode,
      self.window_ext_x,
      self.window_ext_y,
      self.viewport_ext_x,
      self.viewport_ext_y,
    );
    let scale_x = extent_scale_x * page_scale_x * self.playback_scale_x * self.output_scale_x;
    let scale_y = extent_scale_y * page_scale_y * self.playback_scale_y * self.output_scale_y;
    (mapped_x * scale_x, mapped_y * scale_y)
  }

  fn mapped_horizontal_distance(&self, logical_width: i64) -> f32 {
    let width = i32::try_from(logical_width).unwrap_or(if logical_width < 0 {
      i32::MIN
    } else {
      i32::MAX
    });
    // ExtTextOut maps the cumulative logical origin and the zero origin to
    // device coordinates separately, rounds both, and then subtracts them.
    // Keeping that phase also retains the outer playback viewport.
    let origin = self.map_point(EmfPoint { x: 0, y: 0 });
    let endpoint = self.map_point(EmfPoint { x: width, y: 0 });
    let x = endpoint.0.round() - origin.0.round();
    let y = endpoint.1.round() - origin.1.round();
    x.hypot(y).copysign(width as f32)
  }

  fn draw_text(&mut self, x: i32, y: i32, text: &str, color: EmfColor, height: i32) {
    self.draw_text_with_font(
      x,
      y,
      text,
      color,
      &WmfTextFont {
        height,
        family: None,
        char_set: 0,
        weight: 400,
        italic: false,
        quality: crate::wmf::WmfFontQuality::Default.raw(),
      },
    );
  }

  fn draw_text_with_font(
    &mut self,
    x: i32,
    y: i32,
    text: &str,
    color: EmfColor,
    font: &WmfTextFont,
  ) {
    self.draw_wmf_text(x, y, text, color, font, None);
  }

  fn draw_wmf_text(
    &mut self,
    x: i32,
    y: i32,
    text: &str,
    color: EmfColor,
    font: &WmfTextFont,
    logical_advances: Option<&[i16]>,
  ) {
    let (mapped_x, mapped_y) = self.map_point(EmfPoint { x, y });
    let height = self
      .mapped_vertical_length(if font.height == 0 { 12 } else { font.height })
      .round()
      .max(1.0);
    let advances = logical_advances.map(|values| {
      let values = values
        .iter()
        .map(|value| i32::from(*value))
        .collect::<Vec<_>>();
      cumulative_mapped_advances(&values, |logical_cumulative| {
        self.mapped_horizontal_distance(logical_cumulative)
      })
    });
    self.draw_text_at_device(
      color,
      TextRenderRequest {
        font,
        text,
        x: mapped_x.round(),
        baseline_y: mapped_y.round(),
        height,
        horizontal_scale: 1.0,
        advances: advances.as_deref(),
        surface: self.text_surface,
      },
    );
  }

  fn draw_emf_text(
    &mut self,
    text_record: ExtTextRecord,
    text: &str,
    color: EmfColor,
    font: &WmfTextFont,
    logical_advances: Option<&[i32]>,
    logical_displacement: Option<EmfPoint>,
  ) {
    let font_height = if font.height == 0 { 12 } else { font.height };
    let logical_width = logical_displacement.map(|displacement| displacement.x);
    let update_current_position = self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::UPDATE_CP);
    let reference = if update_current_position {
      self.current_pos
    } else {
      EmfPoint {
        x: text_record.x,
        y: text_record.y,
      }
    };
    let aligned_x = if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::CENTER)
    {
      reference
        .x
        .saturating_sub(logical_width.unwrap_or_default() / 2)
    } else if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::RIGHT)
    {
      reference
        .x
        .saturating_sub(logical_width.unwrap_or_default())
    } else {
      reference.x
    };
    let (mapped_x, reference_y) = self.map_point(EmfPoint {
      x: aligned_x,
      y: reference.y,
    });
    // GDI maps the logical text reference point and LOGFONT character height
    // to device units before selecting and rasterizing the realized font.
    let mapped_x = mapped_x.round();
    let reference_y = reference_y.round();
    // GDI realizes a transformed LOGFONT height in whole device pixels by
    // truncating the positive magnitude. For example, the -11 logical Segoe
    // UI font in tdf135653 maps to 22.83 pixels and Windows uses a 22-pixel
    // realization; rounding to 23 grows every glyph one row above TA_TOP.
    let height = self.mapped_vertical_length(font_height).floor().max(1.0);
    let mapped_y =
      self
        .font_cache
        .baseline_for_alignment(font, height, reference_y, self.text_alignment);
    let advances = logical_advances.map(|values| {
      cumulative_mapped_advances(values, |logical_cumulative| {
        self.mapped_horizontal_distance(logical_cumulative)
      })
    });
    // [MS-EMF] 2.3.5.8 defines exScale/eyScale as page-space to physical
    // (.01mm) scales for GM_COMPATIBLE text. Convert equal physical X/Y font
    // dimensions through the player's full output mapping; supplied Dx
    // positions remain independent page-space origins. This matters when an
    // OLE preview is played into a host viewport with a different aspect
    // ratio from its recorded device.
    let horizontal_scale = if text_record.graphics_mode == 1
      && text_record.x_scale.is_finite()
      && text_record.y_scale.is_finite()
      && text_record.x_scale != 0.0
      && text_record.y_scale != 0.0
    {
      let output_axis_ratio =
        self.mapped_horizontal_length(font_height) / self.mapped_vertical_length(font_height);
      output_axis_ratio * (text_record.y_scale / text_record.x_scale).abs()
    } else {
      1.0
    };
    self.draw_text_at_device(
      color,
      TextRenderRequest {
        font,
        text,
        x: mapped_x,
        baseline_y: mapped_y,
        height,
        horizontal_scale,
        advances: advances.as_deref(),
        surface: self.text_surface,
      },
    );
    if update_current_position && let Some(displacement) = logical_displacement {
      self.current_pos = EmfPoint {
        x: aligned_x.saturating_add(displacement.x),
        y: reference.y.saturating_add(displacement.y),
      };
    }
  }

  fn draw_text_at_device(&mut self, color: EmfColor, request: TextRenderRequest<'_>) {
    if self.suppress_text {
      return;
    }
    if let Some(glyphs) = self.font_cache.render_text(&request) {
      for glyph in &glyphs {
        self.draw_gdi_glyph(glyph, color);
      }
      return;
    }

    let scale = ((request.height as usize).max(7) / 7).max(1);
    let mut cursor_x = request.x.round() as i32;
    let baseline_y = request.baseline_y.round() as i32;
    for (index, ch) in request.text.chars().enumerate() {
      let advance = request
        .advances
        .and_then(|values| values.get(index))
        .copied()
        .unwrap_or((6 * scale) as f32)
        .round() as i32;
      if ch.is_whitespace() {
        cursor_x += request
          .advances
          .and_then(|values| values.get(index))
          .copied()
          .unwrap_or((4 * scale) as f32)
          .round() as i32;
        continue;
      }
      draw_glyph_5x7(
        self,
        cursor_x,
        baseline_y - (7 * scale) as i32,
        ch,
        color,
        scale,
      );
      cursor_x += advance;
    }
  }

  fn draw_gdi_glyph(&mut self, glyph: &RenderedGlyph, color: EmfColor) {
    for y in 0..glyph.height {
      for x in 0..glyph.width {
        let device_x = glyph.left + x as i32;
        let device_y = glyph.top + y as i32;
        match &glyph.mask {
          RenderedGlyphMask::Monochrome { stride, bits } => {
            if bits[y * stride + x / 8] & (0x80 >> (x % 8)) != 0 {
              self.set_vector_pixel(device_x, device_y, color);
            }
          }
          RenderedGlyphMask::Grayscale(coverage) => {
            let level = (u16::from(coverage[y * glyph.width + x]) * 16 + 127) / 255;
            if level <= 1 {
              continue;
            }
            let Some(destination) = self.pixel(device_x, device_y) else {
              continue;
            };
            let blended = if level >= 16 {
              color
            } else {
              gdi_grayscale_blend(destination, color, level as usize)
            };
            self.set_vector_pixel(device_x, device_y, blended);
          }
          RenderedGlyphMask::Lcd(coverage) => {
            let coverage = coverage[y * glyph.width + x];
            if coverage == [0; 3] {
              continue;
            }
            let Some(destination) = self.pixel(device_x, device_y) else {
              continue;
            };
            self.set_vector_pixel(
              device_x,
              device_y,
              EmfColor {
                r: gdi_subpixel_blend(destination.r, color.r, coverage[0]),
                g: gdi_subpixel_blend(destination.g, color.g, coverage[1]),
                b: gdi_subpixel_blend(destination.b, color.b, coverage[2]),
              },
            );
          }
        }
      }
    }
  }

  fn fill_arc_segment(
    &mut self,
    rect: (i32, i32, i32, i32),
    start_angle: f32,
    sweep_angle: f32,
    pie: bool,
  ) {
    let (left, top, right, bottom) = rect;
    let points = arc_segment_points(left, top, right, bottom, start_angle, sweep_angle, pie);
    if pie {
      self.fill_polygon(&points);
      self.draw_polyline(&points, true);
    } else {
      self.draw_polyline(&points, false);
    }
  }

  fn save_state(&mut self) {
    self.saved_states.push(self.snapshot());
  }

  fn snapshot(&self) -> EmfVectorSnapshot {
    EmfVectorSnapshot {
      map_mode: self.map_mode,
      window_org_x: self.window_org_x,
      window_org_y: self.window_org_y,
      window_ext_x: self.window_ext_x,
      window_ext_y: self.window_ext_y,
      viewport_org_x: self.viewport_org_x,
      viewport_org_y: self.viewport_org_y,
      viewport_ext_x: self.viewport_ext_x,
      viewport_ext_y: self.viewport_ext_y,
      world_transform: self.world_transform,
      emf_plus_page_unit: self.emf_plus_page_unit,
      emf_plus_page_scale: self.emf_plus_page_scale,
      current_brush: self.current_brush,
      current_brush_is_solid: self.current_brush_is_solid,
      current_pen: self.current_pen,
      current_font: self.current_font,
      current_pos: self.current_pos,
      text_color: self.text_color,
      binary_raster_operation: self.binary_raster_operation,
      text_alignment: self.text_alignment,
      clip_rect: self.clip_rect,
      clip_mask: self.clip_mask.clone(),
    }
  }

  fn restore_state(&mut self) {
    let Some(saved) = self.saved_states.pop() else {
      return;
    };
    self.restore_snapshot(saved);
  }

  fn restore_snapshot(&mut self, saved: EmfVectorSnapshot) {
    self.map_mode = saved.map_mode;
    self.window_org_x = saved.window_org_x;
    self.window_org_y = saved.window_org_y;
    self.window_ext_x = saved.window_ext_x;
    self.window_ext_y = saved.window_ext_y;
    self.viewport_org_x = saved.viewport_org_x;
    self.viewport_org_y = saved.viewport_org_y;
    self.viewport_ext_x = saved.viewport_ext_x;
    self.viewport_ext_y = saved.viewport_ext_y;
    self.world_transform = saved.world_transform;
    self.emf_plus_page_unit = saved.emf_plus_page_unit;
    self.emf_plus_page_scale = saved.emf_plus_page_scale;
    self.current_brush = saved.current_brush;
    self.current_brush_is_solid = saved.current_brush_is_solid;
    self.current_pen = saved.current_pen;
    self.current_font = saved.current_font;
    self.current_pos = saved.current_pos;
    self.text_color = saved.text_color;
    self.binary_raster_operation = saved.binary_raster_operation;
    self.text_alignment = saved.text_alignment;
    self.clip_rect = saved.clip_rect;
    self.clip_mask = saved.clip_mask;
  }

  fn save_emf_plus_state(&mut self, stack_index: u32, container: bool) {
    let snapshot = self.snapshot();
    let stack = if container {
      &mut self.emf_plus_containers
    } else {
      &mut self.emf_plus_saved_states
    };
    stack.retain(|(index, _)| *index != stack_index);
    stack.push((stack_index, snapshot));
  }

  fn restore_emf_plus_state(&mut self, stack_index: u32, container: bool) {
    let snapshot = {
      let stack = if container {
        &mut self.emf_plus_containers
      } else {
        &mut self.emf_plus_saved_states
      };
      let Some(position) = stack.iter().rposition(|(index, _)| *index == stack_index) else {
        return;
      };
      let snapshot = stack[position].1.clone();
      // Restore/EndContainer pops the addressed level and every level
      // nested after it; Save and container stacks are independent.
      stack.truncate(position);
      snapshot
    };
    self.restore_snapshot(snapshot);
  }

  fn begin_emf_device_context(&mut self) -> EmfDeviceContextBridge {
    let bridge = EmfDeviceContextBridge {
      snapshot: self.snapshot(),
      saved_states: std::mem::take(&mut self.saved_states),
      brush_colors: std::mem::take(&mut self.brush_colors),
      solid_brushes: std::mem::take(&mut self.solid_brushes),
      pens: std::mem::take(&mut self.pens),
      fonts: std::mem::take(&mut self.fonts),
    };

    // [MS-EMFPLUS] 2.3.3.2 hands subsequent EMF records a device
    // context. GDI mapping and SaveDC state start in device coordinates;
    // they must not inherit the EMF+ world transform or graphics-state
    // stack. The active clip is deliberately retained for the borrowed DC.
    self.map_mode = EmrMapMode::Text;
    self.window_org_x = 0;
    self.window_org_y = 0;
    self.window_ext_x = self.natural_width.max(1) as i32;
    self.window_ext_y = self.natural_height.max(1) as i32;
    self.viewport_org_x = 0;
    self.viewport_org_y = 0;
    self.viewport_ext_x = self.natural_width.max(1) as i32;
    self.viewport_ext_y = self.natural_height.max(1) as i32;
    self.world_transform = EmfTransform::identity();
    self.emf_plus_page_unit = EmfPlusUnitType::Pixel;
    self.emf_plus_page_scale = 1.0;
    self.current_brush = None;
    self.current_brush_is_solid = false;
    self.current_pen = Some(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width: 1,
      width_space: EmfPenWidthSpace::Device,
    });
    self.current_font = None;
    self.current_pos = EmfPoint { x: 0, y: 0 };
    self.text_color = EmfColor { r: 0, g: 0, b: 0 };
    self.binary_raster_operation = WmfBinaryRasterOperation::CopyPen;
    self.text_alignment = WmfTextAlignmentModeFlags::empty();
    bridge
  }

  fn end_emf_device_context(&mut self, bridge: EmfDeviceContextBridge) {
    self.restore_snapshot(bridge.snapshot);
    self.saved_states = bridge.saved_states;
    self.brush_colors = bridge.brush_colors;
    self.solid_brushes = bridge.solid_brushes;
    self.pens = bridge.pens;
    self.fonts = bridge.fonts;
  }

  fn set_clip_rect_logical(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let (x1, y1) = self.map_point(EmfPoint { x: left, y: top });
    let (x2, y2) = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let rect = (
      x1.min(x2).floor().max(0.0) as i32,
      y1.min(y2).floor().max(0.0) as i32,
      x1.max(x2).ceil().min(self.width as f32) as i32,
      y1.max(y2).ceil().min(self.height as f32) as i32,
    );
    self.set_clip_rect_device(rect, 0);
  }

  fn set_clip_rect_device(&mut self, rect: (i32, i32, i32, i32), combine_mode: u8) {
    let next = (
      rect.0.clamp(0, self.width as i32),
      rect.1.clamp(0, self.height as i32),
      rect.2.clamp(0, self.width as i32),
      rect.3.clamp(0, self.height as i32),
    );
    if combine_mode == 0 {
      self.clip_rect = Some(next);
      self.clip_mask = None;
      return;
    }
    if combine_mode == 1 && self.clip_mask.is_none() {
      self.clip_rect = Some(match self.clip_rect {
        Some(current) => intersect_rects(current, next),
        None => next,
      });
      return;
    }
    let mask = self.rect_clip_mask(next);
    self.combine_clip_mask(mask, combine_mode);
  }

  fn set_clip_points_logical(&mut self, points: &[EmfPoint], combine_mode: u8) {
    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    if let Some(rect) = axis_aligned_clip_rect(&mapped, self.width, self.height) {
      self.set_clip_rect_device(rect, combine_mode);
      return;
    }
    let mask = self.polygon_mask(&mapped);
    self.combine_clip_mask(mask, combine_mode);
  }

  fn set_clip_region(&mut self, region: &EmfPlusRenderRegion, combine_mode: u8) {
    let next = self.emf_plus_region_mask(region);
    self.combine_clip_region_mask(next, combine_mode);
  }

  fn fill_emf_plus_region(&mut self, region: &EmfPlusRenderRegion, brush: &EmfPlusRenderBrush) {
    let mask = self.emf_plus_region_mask(region);
    for y in 0..self.height {
      for x in 0..self.width {
        if mask.as_ref().is_some_and(|mask| !mask[y * self.width + x]) {
          continue;
        }
        let color = brush.color_at(x as i32, y as i32);
        self.set_pixel_with_alpha(x as i32, y as i32, color.color, color.alpha);
      }
    }
  }

  /// Returns `None` for an infinite region and a device-space mask for
  /// every finite region, including an all-false mask for an empty region.
  fn emf_plus_region_mask(&self, region: &EmfPlusRenderRegion) -> Option<Vec<bool>> {
    match region {
      EmfPlusRenderRegion::Empty => Some(vec![false; self.width * self.height]),
      EmfPlusRenderRegion::Infinite => None,
      EmfPlusRenderRegion::Polygon(points) => {
        let mapped = points
          .iter()
          .map(|point| self.map_point(*point))
          .collect::<Vec<_>>();
        Some(
          axis_aligned_clip_rect(&mapped, self.width, self.height).map_or_else(
            || self.polygon_mask(&mapped),
            |rect| self.rect_clip_mask(rect),
          ),
        )
      }
      EmfPlusRenderRegion::Combine { mode, left, right } => {
        let left = self.emf_plus_region_mask(left);
        let right = self.emf_plus_region_mask(right);
        self.combine_region_masks(left, right, *mode)
      }
    }
  }

  fn offset_clip(&mut self, dx: f32, dy: f32) {
    if let Some((left, top, right, bottom)) = self.clip_rect {
      self.clip_rect = Some((
        (left as f32 + dx).round() as i32,
        (top as f32 + dy).round() as i32,
        (right as f32 + dx).round() as i32,
        (bottom as f32 + dy).round() as i32,
      ));
    }
    if let Some(mask) = self.clip_mask.take() {
      let mut shifted = vec![false; mask.len()];
      let dx = dx.round() as i32;
      let dy = dy.round() as i32;
      for y in 0..self.height {
        for x in 0..self.width {
          if !mask[y * self.width + x] {
            continue;
          }
          let nx = x as i32 + dx;
          let ny = y as i32 + dy;
          if nx >= 0 && ny >= 0 && nx < self.width as i32 && ny < self.height as i32 {
            shifted[ny as usize * self.width + nx as usize] = true;
          }
        }
      }
      self.clip_mask = Some(shifted);
      self.update_clip_rect_from_mask();
    }
  }

  fn combine_clip_mask(&mut self, next: Vec<bool>, combine_mode: u8) {
    self.combine_clip_region_mask(Some(next), combine_mode);
  }

  fn combine_clip_region_mask(&mut self, next: Option<Vec<bool>>, combine_mode: u8) {
    let current = match self.clip_mask.take() {
      Some(mask) => Some(mask),
      None => self.clip_rect.map(|rect| self.rect_clip_mask(rect)),
    };
    let mask = self.combine_region_masks(current, next, combine_mode);
    self.clip_mask = mask;
    self.update_clip_rect_from_mask();
  }

  fn combine_region_masks(
    &self,
    left: Option<Vec<bool>>,
    right: Option<Vec<bool>>,
    combine_mode: u8,
  ) -> Option<Vec<bool>> {
    let empty = || vec![false; self.width * self.height];
    match (left, right, combine_mode) {
      (_, right, 0) => right,
      (None, right, 1) => right,
      (left, None, 1) => left,
      (Some(left), Some(right), 1) => Some(
        left
          .into_iter()
          .zip(right)
          .map(|(left, right)| left && right)
          .collect(),
      ),
      (None, _, 2) | (_, None, 2) => None,
      (Some(left), Some(right), 2) => Some(
        left
          .into_iter()
          .zip(right)
          .map(|(left, right)| left || right)
          .collect(),
      ),
      (None, None, 3) => Some(empty()),
      (None, Some(mask), 3) | (Some(mask), None, 3) => {
        Some(mask.into_iter().map(|value| !value).collect())
      }
      (Some(left), Some(right), 3) => Some(
        left
          .into_iter()
          .zip(right)
          .map(|(left, right)| left ^ right)
          .collect(),
      ),
      (None, None, 4) | (Some(_), None, 4) => Some(empty()),
      (None, Some(right), 4) => Some(right.into_iter().map(|value| !value).collect()),
      (Some(left), Some(right), 4) => Some(
        left
          .into_iter()
          .zip(right)
          .map(|(left, right)| left && !right)
          .collect(),
      ),
      (None, _, 5) => Some(empty()),
      (Some(left), None, 5) => Some(left.into_iter().map(|value| !value).collect()),
      (Some(left), Some(right), 5) => Some(
        left
          .into_iter()
          .zip(right)
          .map(|(left, right)| right && !left)
          .collect(),
      ),
      (_, right, _) => right,
    }
  }

  fn rect_clip_mask(&self, rect: (i32, i32, i32, i32)) -> Vec<bool> {
    let mut mask = vec![false; self.width * self.height];
    for y in rect.1.max(0) as usize..rect.3.max(0) as usize {
      let row = y * self.width;
      for x in rect.0.max(0) as usize..rect.2.max(0) as usize {
        mask[row + x] = true;
      }
    }
    mask
  }

  fn update_clip_rect_from_mask(&mut self) {
    let Some(mask) = &self.clip_mask else {
      self.clip_rect = None;
      return;
    };
    let mut left = self.width;
    let mut top = self.height;
    let mut right = 0usize;
    let mut bottom = 0usize;
    for y in 0..self.height {
      for x in 0..self.width {
        if !mask[y * self.width + x] {
          continue;
        }
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + 1);
        bottom = bottom.max(y + 1);
      }
    }
    self.clip_rect = (right > left && bottom > top).then_some((
      left as i32,
      top as i32,
      right as i32,
      bottom as i32,
    ));
  }

  fn polygon_mask(&self, mapped: &[(f32, f32)]) -> Vec<bool> {
    let mut mask = vec![false; self.width * self.height];
    if mapped.len() < 3 {
      return mask;
    }
    visit_polygon_scanline_spans(mapped, self.width, self.height, |y, start, end| {
      for x in start..end {
        mask[y * self.width + x] = true;
      }
    });
    mask
  }

  fn fill_polygon(&mut self, points: &[EmfPoint]) {
    let Some(color) = self.current_brush else {
      return;
    };
    if points.len() < 3 {
      return;
    }

    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    let width = self.width;
    let height = self.height;
    visit_polygon_scanline_spans(&mapped, width, height, |y, start, end| {
      for x in start..end {
        self.set_vector_pixel(x as i32, y as i32, color);
      }
    });
  }

  fn fill_polygon_with_emf_plus_brush(&mut self, points: &[EmfPoint], brush: &EmfPlusRenderBrush) {
    if points.len() < 3 {
      return;
    }

    let mapped = points
      .iter()
      .map(|point| self.map_point(*point))
      .collect::<Vec<_>>();
    let width = self.width;
    let height = self.height;
    visit_polygon_scanline_spans(&mapped, width, height, |y, start, end| {
      for x in start..end {
        let color = brush.color_at(x as i32, y as i32);
        self.set_pixel_with_alpha(x as i32, y as i32, color.color, color.alpha);
      }
    });
  }

  fn draw_polyline(&mut self, points: &[EmfPoint], closed: bool) {
    let Some(pen) = self.current_pen else {
      return;
    };
    if points.len() < 2 {
      return;
    }
    let pen = self.resolve_pen(pen);
    let mut coverage = vec![false; self.width * self.height];
    for pair in points.windows(2) {
      self.mark_line_coverage(pair[0], pair[1], pen, &mut coverage);
    }
    if closed {
      self.mark_line_coverage(points[points.len() - 1], points[0], pen, &mut coverage);
    }
    self.paint_pen_coverage(&coverage, pen);
  }

  fn mark_line_coverage(&self, a: EmfPoint, b: EmfPoint, pen: EmfPen, coverage: &mut [bool]) {
    if self.width == 0 || self.height == 0 {
      return;
    }
    let radius = (pen.width.max(1) / 2) as f64;
    let canvas = (0, 0, self.width as i32, self.height as i32);
    let (left, top, right, bottom) = self
      .clip_rect
      .map_or(canvas, |clip_rect| intersect_rects(canvas, clip_rect));
    if right <= left || bottom <= top {
      return;
    }
    let (x0, y0) = self.map_point(a);
    let (x1, y1) = self.map_point(b);
    let Some(((x0, y0), (x1, y1))) = clip_line_to_rect(
      (f64::from(x0), f64::from(y0)),
      (f64::from(x1), f64::from(y1)),
      (
        f64::from(left) - radius,
        f64::from(top) - radius,
        f64::from(right - 1) + radius,
        f64::from(bottom - 1) + radius,
      ),
    ) else {
      return;
    };
    let mut x0 = x0.round() as i32;
    let mut y0 = y0.round() as i32;
    let x1 = x1.round() as i32;
    let y1 = y1.round() as i32;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    let pen_radius = (pen.width.max(1) / 2) as i32;
    loop {
      for y in y0 - pen_radius..=y0 + pen_radius {
        for x in x0 - pen_radius..=x0 + pen_radius {
          if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
            coverage[y as usize * self.width + x as usize] = true;
          }
        }
      }
      if x0 == x1 && y0 == y1 {
        break;
      }
      let e2 = 2 * error;
      if e2 >= dy {
        error += dy;
        x0 += sx;
      }
      if e2 <= dx {
        error += dx;
        y0 += sy;
      }
    }
  }

  fn paint_pen_coverage(&mut self, coverage: &[bool], pen: EmfPen) {
    for y in 0..self.height {
      for x in 0..self.width {
        if coverage[y * self.width + x] {
          self.set_vector_pixel_with_alpha(x as i32, y as i32, pen.color, pen.alpha);
        }
      }
    }
  }

  fn fill_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let points = [
      EmfPoint { x: left, y: top },
      EmfPoint { x: right, y: top },
      EmfPoint {
        x: right,
        y: bottom,
      },
      EmfPoint { x: left, y: bottom },
    ];
    self.fill_polygon(&points);
    self.draw_polyline(&points, true);
  }

  fn fill_solid_rect(&mut self, left: i32, top: i32, right: i32, bottom: i32, color: EmfColor) {
    let (mapped_left, mapped_top) = self.map_point(EmfPoint { x: left, y: top });
    let (mapped_right, mapped_bottom) = self.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let left = mapped_left.min(mapped_right).floor().max(0.0) as i32;
    let top = mapped_top.min(mapped_bottom).floor().max(0.0) as i32;
    let right = mapped_left.max(mapped_right).ceil().min(self.width as f32) as i32;
    let bottom = mapped_top.max(mapped_bottom).ceil().min(self.height as f32) as i32;
    for y in top..bottom {
      for x in left..right {
        self.set_vector_pixel(x, y, color);
      }
    }
  }

  fn fill_ellipse(&mut self, left: i32, top: i32, right: i32, bottom: i32) {
    let steps = 72;
    let cx = (left + right) as f32 / 2.0;
    let cy = (top + bottom) as f32 / 2.0;
    let rx = (right - left).abs() as f32 / 2.0;
    let ry = (bottom - top).abs() as f32 / 2.0;
    let mut points = Vec::with_capacity(steps);
    for index in 0..steps {
      let theta = index as f32 * std::f32::consts::TAU / steps as f32;
      points.push(EmfPoint {
        x: (cx + theta.cos() * rx).round() as i32,
        y: (cy + theta.sin() * ry).round() as i32,
      });
    }
    self.fill_polygon(&points);
    self.draw_polyline(&points, true);
  }

  fn fill_ellipse_with_emf_plus_brush(
    &mut self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    brush: &EmfPlusRenderBrush,
  ) {
    let steps = 72;
    let cx = (left + right) as f32 / 2.0;
    let cy = (top + bottom) as f32 / 2.0;
    let rx = (right - left).abs() as f32 / 2.0;
    let ry = (bottom - top).abs() as f32 / 2.0;
    let mut points = Vec::with_capacity(steps);
    for index in 0..steps {
      let theta = index as f32 * std::f32::consts::TAU / steps as f32;
      points.push(EmfPoint {
        x: (cx + theta.cos() * rx).round() as i32,
        y: (cy + theta.sin() * ry).round() as i32,
      });
    }
    self.fill_polygon_with_emf_plus_brush(&points, brush);
    self.draw_polyline(&points, true);
  }

  fn select_object(&mut self, object_id: u32) {
    match object_id {
      WHITE_BRUSH => {
        self.current_brush = Some(EmfColor {
          r: 255,
          g: 255,
          b: 255,
        });
        self.current_brush_is_solid = true;
      }
      BLACK_BRUSH => {
        self.current_brush = Some(EmfColor { r: 0, g: 0, b: 0 });
        self.current_brush_is_solid = true;
      }
      NULL_BRUSH => {
        self.current_brush = None;
        self.current_brush_is_solid = false;
      }
      0x8000_0001..=0x8000_0003 => {
        // Stock gray brush colors are device-dependent. Rasterize them.
        self.current_brush_is_solid = false;
      }
      WHITE_PEN => {
        self.current_pen = Some(EmfPen {
          color: EmfColor {
            r: 255,
            g: 255,
            b: 255,
          },
          alpha: u8::MAX,
          width: 1,
          width_space: EmfPenWidthSpace::Device,
        })
      }
      BLACK_PEN => {
        self.current_pen = Some(EmfPen {
          color: EmfColor { r: 0, g: 0, b: 0 },
          alpha: u8::MAX,
          width: 1,
          width_space: EmfPenWidthSpace::Device,
        })
      }
      NULL_PEN => self.current_pen = None,
      _ => {
        if let Some(brush) = self.brush_colors.get(&object_id).copied() {
          self.current_brush = Some(brush);
          self.current_brush_is_solid = self.solid_brushes.contains(&object_id);
        }
        if let Some(pen) = self.pens.get(&object_id).copied() {
          self.current_pen = pen;
        }
        if self.fonts.contains_key(&object_id) {
          self.current_font = Some(object_id);
        }
      }
    }
  }
}

fn decode_vector_emf_as_png(
  data: &[u8],
  options: RenderOptions,
  text_surface: GdiTextSurface,
) -> Result<DecodedMetafile, String> {
  let mut state = if text_surface == GdiTextSurface::Color {
    EmfVectorState::new_with_options(data, options)?
  } else {
    EmfVectorState::new_with_options_and_text_surface(data, options, text_surface)?
  };
  let mut pos =
    emf_header_record_size(data).ok_or_else(|| "invalid EMF header record".to_string())?;
  let mut emf_plus_playback = false;
  let mut emf_device_context = None;

  while pos + EMF_RECORD_HEADER_SIZE <= data.len() {
    let record_type = read_u32(data, pos)?;
    let record_size = read_u32(data, pos + 4)? as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || pos + record_size > data.len() {
      return Err(format!(
        "invalid EMF record at offset {pos}: type=0x{record_type:08x} size={record_size}"
      ));
    }
    let is_emf_plus_comment =
      record_type == EMR_COMMENT && emf_comment_is_emf_plus(data, pos, record_size);
    if is_emf_plus_comment && let Some(bridge) = emf_device_context.take() {
      // The next EMF+ record ends the GetDC interval. Restore the EMF+
      // graphics state before consuming that record.
      state.end_emf_device_context(bridge);
    }
    if emf_plus_playback
      && emf_device_context.is_none()
      && record_type != EMR_COMMENT
      && record_type != EMR_EOF
    {
      // [MS-EMFPLUS] 1.3: EMF+ playback ignores the complete classic EMF
      // fallback in both Only and Dual metafiles. Classic drawing records
      // are consumed only inside an explicit EmfPlusGetDC interval.
      pos += record_size;
      continue;
    }
    let mut consumed_following_record_size = 0usize;

    match record_type {
      EMR_SET_WINDOW_ORG_EX if record_size >= 16 => {
        state.window_org_x = read_i32(data, pos + 8)?;
        state.window_org_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_WINDOW_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.window_ext_x = nonzero_mapping_extent(read_i32(data, pos + 8)?);
          state.window_ext_y = nonzero_mapping_extent(read_i32(data, pos + 12)?);
        }
      }
      EMR_SET_VIEWPORT_ORG_EX if record_size >= 16 => {
        state.viewport_org_x = read_i32(data, pos + 8)?;
        state.viewport_org_y = read_i32(data, pos + 12)?;
      }
      EMR_SET_VIEWPORT_EXT_EX if record_size >= 16 => {
        if emf_mapping_extents_are_variable(state.map_mode) {
          state.viewport_ext_x = read_i32(data, pos + 8)?;
          state.viewport_ext_y = read_i32(data, pos + 12)?;
        }
      }
      EMR_SET_MAP_MODE if record_size >= 12 => {
        if let Some(map_mode) = EmrMapMode::from_raw(read_u32(data, pos + 8)?) {
          state.map_mode = map_mode;
        }
      }
      EMR_SET_PIXEL_V if record_size >= 20 => {
        let (x, y) = state.map_point(EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        });
        state.set_pixel(
          x.round() as i32,
          y.round() as i32,
          read_color_ref(data, pos + 16)?,
        );
      }
      EMR_SET_ROP_2 if record_size >= 12 => {
        if let Some(operation) = u16::try_from(read_u32(data, pos + 8)?)
          .ok()
          .and_then(WmfBinaryRasterOperation::from_raw)
        {
          state.binary_raster_operation = operation;
        }
      }
      EMR_MOVE_TO_EX if record_size >= 16 => {
        state.current_pos = EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        };
      }
      EMR_LINE_TO if record_size >= 16 => {
        let next = EmfPoint {
          x: read_i32(data, pos + 8)?,
          y: read_i32(data, pos + 12)?,
        };
        state.draw_polyline(&[state.current_pos, next], false);
        state.current_pos = next;
      }
      EMR_SET_TEXT_COLOR if record_size >= 12 => {
        state.text_color = read_color_ref(data, pos + 8)?;
      }
      EMR_SET_TEXT_ALIGN if record_size >= 12 => {
        state.text_alignment =
          WmfTextAlignmentModeFlags::from_bits_retain(read_u32(data, pos + 8)? as u16);
      }
      EMR_SAVE_DC => state.save_state(),
      EMR_RESTORE_DC => state.restore_state(),
      EMR_SET_WORLD_TRANSFORM if record_size >= 32 => {
        state.world_transform = read_xform(data, pos + 8)?;
      }
      EMR_MODIFY_WORLD_TRANSFORM if record_size >= 36 => {
        let transform = read_xform(data, pos + 8)?;
        let mode = read_u32(data, pos + 32)?;
        state.world_transform = match mode {
          MWT_IDENTITY => EmfTransform::identity(),
          MWT_LEFTMULTIPLY => transform.multiply(state.world_transform),
          MWT_RIGHTMULTIPLY => state.world_transform.multiply(transform),
          MWT_SET => transform,
          _ => state.world_transform,
        };
      }
      EMR_CREATE_PEN if record_size >= 28 => {
        let object_id = read_u32(data, pos + 8)?;
        if object_id & ENHMETA_STOCK_OBJECT == 0 {
          let style = read_u32(data, pos + 12)?;
          let width = read_i32(data, pos + 16)?.unsigned_abs().max(1) as usize;
          state.pens.insert(
            object_id,
            emf_pen_from_style(
              style,
              EmfPen {
                color: read_color_ref(data, pos + 24)?,
                alpha: u8::MAX,
                width,
                width_space: EmfPenWidthSpace::Device,
              },
            ),
          );
        }
      }
      EMR_CREATE_BRUSH_INDIRECT if record_size >= 24 => {
        let object_id = read_u32(data, pos + 8)?;
        state
          .brush_colors
          .insert(object_id, read_color_ref(data, pos + 16)?);
        let brush_style = read_u32(data, pos + 12)
          .ok()
          .and_then(|value| u16::try_from(value).ok())
          .and_then(WmfBrushStyle::from_raw);
        if brush_style == Some(WmfBrushStyle::Solid) {
          state.solid_brushes.insert(object_id);
        } else {
          state.solid_brushes.remove(&object_id);
        }
      }
      // [MS-EMF] 2.3.7.9 permits an empty LogPenEx StyleEntry array.  The
      // fixed record is therefore 52 bytes (8-byte EMR header, five DWORDs,
      // and the 24-byte fixed LogPenEx), as emitted by Adobe in Apache POI's
      // WithDrawing.xlsx.  Requiring one nonexistent style entry loses a
      // legal PS_NULL pen and incorrectly outlines every following polygon.
      EMR_EXT_CREATE_PEN if record_size >= 52 => {
        let object_id = read_u32(data, pos + 8)?;
        if object_id & ENHMETA_STOCK_OBJECT == 0 {
          let style = read_u32(data, pos + 28)?;
          let width = read_u32(data, pos + 32)?.max(1) as usize;
          state.pens.insert(
            object_id,
            emf_pen_from_style(
              style,
              EmfPen {
                color: read_color_ref(data, pos + 40)?,
                alpha: u8::MAX,
                width,
                width_space: EmfPenWidthSpace::Device,
              },
            ),
          );
        }
      }
      EMR_EXT_CREATE_FONT_INDIRECT_W if record_size >= 104 => {
        if let Some((object_id, font)) = read_logfont_object(data, pos, record_size)
          && object_id & ENHMETA_STOCK_OBJECT == 0
        {
          state.fonts.insert(object_id, font);
        }
      }
      EMR_SELECT_OBJECT if record_size >= 12 => {
        state.select_object(read_u32(data, pos + 8)?);
      }
      EMR_DELETE_OBJECT if record_size >= 12 => {
        let object_id = read_u32(data, pos + 8)?;
        state.brush_colors.remove(&object_id);
        state.solid_brushes.remove(&object_id);
        state.pens.remove(&object_id);
        state.fonts.remove(&object_id);
        if state.current_font == Some(object_id) {
          state.current_font = None;
        }
      }
      EMR_POLYGON if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYBEZIER if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&flatten_bezier_sequence(&points), false);
        }
      }
      EMR_POLYBEZIER_TO if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&flatten_bezier_sequence(&sequence), false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYGON16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYBEZIER16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&flatten_bezier_sequence(&points), false);
        }
      }
      EMR_POLYBEZIER_TO16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&flatten_bezier_sequence(&sequence), false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYLINE if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYLINE_TO if record_size >= 28 => {
        if let Some(points) = read_points_i32(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&sequence, false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYLINE16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYLINE_TO16 if record_size >= 28 => {
        if let Some(points) = read_points_i16(data, pos + 28, read_u32(data, pos + 24)? as usize) {
          let mut sequence = Vec::with_capacity(points.len() + 1);
          sequence.push(state.current_pos);
          sequence.extend_from_slice(&points);
          state.draw_polyline(&sequence, false);
          if let Some(last) = points.last().copied() {
            state.current_pos = last;
          }
        }
      }
      EMR_POLYPOLYLINE if record_size >= 36 => {
        for points in read_poly_polygons_i32(data, pos, record_size)? {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYPOLYGON if record_size >= 36 => {
        for points in read_poly_polygons_i32(data, pos, record_size)? {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_POLYPOLYLINE16 if record_size >= 36 => {
        for points in read_poly_polygons_i16(data, pos, record_size)? {
          state.draw_polyline(&points, false);
        }
      }
      EMR_POLYPOLYGON16 if record_size >= 36 => {
        for points in read_poly_polygons_i16(data, pos, record_size)? {
          state.fill_polygon(&points);
          state.draw_polyline(&points, true);
        }
      }
      EMR_RECTANGLE if record_size >= 24 => {
        state.fill_rect(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ROUND_RECT if record_size >= 32 => {
        state.fill_rect(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ELLIPSE if record_size >= 24 => {
        state.fill_ellipse(
          read_i32(data, pos + 8)?,
          read_i32(data, pos + 12)?,
          read_i32(data, pos + 16)?,
          read_i32(data, pos + 20)?,
        );
      }
      EMR_ARC if record_size >= 40 => {
        let rect = emf_arc_rect(data, pos)?;
        state.fill_arc_segment(
          rect,
          angle_from_emf_arc_point(rect, read_i32(data, pos + 24)?, read_i32(data, pos + 28)?),
          sweep_from_emf_arc_points(data, pos, rect)?,
          false,
        );
      }
      EMR_CHORD | EMR_PIE if record_size >= 40 => {
        let rect = emf_arc_rect(data, pos)?;
        state.fill_arc_segment(
          rect,
          angle_from_emf_arc_point(rect, read_i32(data, pos + 24)?, read_i32(data, pos + 28)?),
          sweep_from_emf_arc_points(data, pos, rect)?,
          true,
        );
      }
      EMR_EXT_TEXTOUT_W => {
        if let Some(text) = extract_emr_ext_text_out_w(data, pos, record_size)
          && let Some(text_record) = ext_text_record(data, pos, record_size)
        {
          let font = emf_current_font(&state);
          let advances = ext_text_advances(data, pos, record_size, text_record);
          let displacement = ext_text_displacement(data, pos, record_size, text_record);
          state.draw_emf_text(
            text_record,
            &text,
            state.text_color,
            &font,
            advances.as_deref(),
            displacement,
          );
        }
      }
      EMR_EXT_TEXTOUT_A => {
        if let Some(text) = extract_emr_ext_text_out_a(data, pos, record_size)
          && let Some(text_record) = ext_text_record(data, pos, record_size)
        {
          let font = emf_current_font(&state);
          let advances = ext_text_advances(data, pos, record_size, text_record);
          let displacement = ext_text_displacement(data, pos, record_size, text_record);
          state.draw_emf_text(
            text_record,
            &text,
            state.text_color,
            &font,
            advances.as_deref(),
            displacement,
          );
        }
      }
      EMR_BIT_BLT | EMR_STRETCH_BLT | EMR_SET_DIBITS_TO_DEVICE | EMR_STRETCH_DIBITS => {
        if let Some(next_record_size) =
          replay_masked_blt_pair(data, pos, record_type, record_size, &mut state)?
        {
          consumed_following_record_size = next_record_size;
        } else if let Some(target) = emf_bitmap_draw_target(data, pos, record_type, record_size)? {
          if let Some(image) = cropped_emf_bitmap(data, pos, record_type, record_size, target)? {
            if let Some(rop) = target.raster_operation {
              state.draw_rgb_image_with_rop(
                target.dest_x,
                target.dest_y,
                target.dest_width,
                target.dest_height,
                &image,
                rop,
              );
            } else {
              state.draw_rgb_image(
                target.dest_x,
                target.dest_y,
                target.dest_width,
                target.dest_height,
                &image,
              );
            }
          } else if let Some(rop) = target.raster_operation
            && !rop.uses_source()
          {
            // [MS-EMF] 2.3.1.2 permits EMR_BITBLT to omit BitmapBuffer
            // whenever its ternary operation does not read the source. GDI
            // control previews use exactly this spelling for brush-backed
            // PATCOPY rectangles: the destination and selected brush remain
            // meaningful even though all four bitmap offsets and sizes are
            // zero.
            let lifted_solid_rect = options.suppress_solid_pattern_rects
              && rop == WmfTernaryRasterOperationCode::PATCOPY
              && state.current_brush_is_solid;
            if !lifted_solid_rect {
              state.fill_rect_with_rop(
                target.dest_x,
                target.dest_y,
                target.dest_x.saturating_add(target.dest_width),
                target.dest_y.saturating_add(target.dest_height),
                rop,
              );
            }
          }
        }
      }
      EMR_ALPHA_BLEND => replay_emf_alpha_blend(data, pos, record_size, &mut state)?,
      EMR_COMMENT if record_size >= 16 => {
        if let Some(control) = process_emf_plus_comment(data, pos, record_size, &mut state)? {
          emf_plus_playback |= control.header;
          if emf_plus_playback && control.get_dc {
            emf_device_context = Some(state.begin_emf_device_context());
          }
        }
      }
      EMR_EOF => break,
      _ => {}
    }

    pos += record_size + consumed_following_record_size;
  }

  Ok(DecodedMetafile {
    data: rgb_to_png(&state.rgb, state.width as u32, state.height as u32)?,
    content_type: "image/png",
  })
}

fn emf_comment_is_emf_plus(data: &[u8], record_offset: usize, record_size: usize) -> bool {
  record_size >= 16
    && read_u32(data, record_offset + 8).is_ok_and(|data_size| data_size >= 4)
    && read_u32(data, record_offset + 12).is_ok_and(|identifier| identifier == EMR_COMMENT_EMFPLUS)
}

fn replay_emf_alpha_blend(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  let record_end = record_offset
    .checked_add(record_size)
    .ok_or_else(|| "EMR_ALPHABLEND range overflows".to_string())?;
  let payload = data
    .get(record_offset + EMF_RECORD_HEADER_SIZE..record_end)
    .ok_or_else(|| "EMR_ALPHABLEND points outside the metafile".to_string())?;
  let record = EmfRecordRef {
    record_type: EMR_ALPHA_BLEND,
    data: payload,
  };
  let EmfRecordData::AlphaBlend(value) = record.parse_data().map_err(|error| error.to_string())?
  else {
    return Ok(());
  };
  let Some(image) = emf_alpha_blend_raster(&value)? else {
    return Ok(());
  };
  state.draw_alpha_blended_image(
    value.dest.x,
    value.dest.y,
    value.dest_size.cx,
    value.dest_size.cy,
    &image,
    value.blend_function.source_constant_alpha,
  );
  Ok(())
}

fn emf_alpha_blend_raster(value: &EmrAlphaBlend) -> Result<Option<AlphaBlendRaster>, String> {
  let Some(bitmap) = value.bitmap.as_ref() else {
    return Ok(None);
  };
  let dib = bitmap
    .device_independent_bitmap()
    .map_err(|error| error.to_string())?;
  let color_usage = value
    .color_usage_kind()
    .ok_or_else(|| "EMR_ALPHABLEND has an unsupported DIB color usage".to_string())?;
  let Some(pixels) = device_independent_bitmap_to_rgb(&dib, color_usage, None)? else {
    return Ok(None);
  };
  let source_alpha = match value.blend_function.alpha_format_kind() {
    Some(EmrAlphaFormat::ConstantAlpha) => None,
    Some(EmrAlphaFormat::SourceAlpha) => Some(dib_source_alpha_plane(&dib)?),
    None => return Ok(None),
  };

  // XformSrc belongs to the source DC. Win32 maps the two logical source
  // rectangle corners to device coordinates before validating and sampling
  // the selected DIB; negative physical source extents make AlphaBlend fail.
  let transform = EmfTransform {
    m11: value.xform_source.m11,
    m12: value.xform_source.m12,
    m21: value.xform_source.m21,
    m22: value.xform_source.m22,
    dx: value.xform_source.dx,
    dy: value.xform_source.dy,
  };
  let first = transform.apply(EmfPoint {
    x: value.source.x,
    y: value.source.y,
  });
  let second = transform.apply(EmfPoint {
    x: value.source.x.saturating_add(value.source_size.cx),
    y: value.source.y.saturating_add(value.source_size.cy),
  });
  if ![first.0, first.1, second.0, second.1]
    .into_iter()
    .all(f32::is_finite)
  {
    return Ok(None);
  }
  let left = first.0.round() as i32;
  let top = first.1.round() as i32;
  let right = second.0.round() as i32;
  let bottom = second.1.round() as i32;
  let Some(width) = right.checked_sub(left).filter(|width| *width > 0) else {
    return Ok(None);
  };
  let Some(height) = bottom.checked_sub(top).filter(|height| *height > 0) else {
    return Ok(None);
  };

  Ok(crop_alpha_blend_raster(
    AlphaBlendRaster {
      pixels,
      source_alpha,
    },
    (left, top, width, height),
  ))
}

fn dib_source_alpha_plane(dib: &DeviceIndependentBitmap) -> Result<Vec<u8>, String> {
  let header = &dib.info.header;
  if header.bit_count() != 32 {
    return Err(format!(
      "EMR_ALPHABLEND AC_SRC_ALPHA requires a 32-bpp source, got {} bpp",
      header.bit_count()
    ));
  }
  let width = usize::try_from(header.width())
    .map_err(|_| "EMR_ALPHABLEND source width is negative".to_string())?;
  let height = header.height_abs() as usize;
  let stride = header
    .scan_line_stride_bytes()
    .map_err(|error| error.to_string())? as usize;
  let required = stride
    .checked_mul(height)
    .ok_or_else(|| "EMR_ALPHABLEND source dimensions overflow".to_string())?;
  if dib.bits.len() < required {
    return Err(format!(
      "EMR_ALPHABLEND source bits are truncated: need {required}, got {}",
      dib.bits.len()
    ));
  }
  let alpha_mask = match header {
    DibHeader::V4(value) if value.alpha_mask != 0 => value.alpha_mask,
    DibHeader::V5(value) if value.v4.alpha_mask != 0 => value.v4.alpha_mask,
    // BLENDFUNCTION's AC_SRC_ALPHA contract is A8R8G8B8. A 40-byte
    // BITMAPINFOHEADER has only the three RGB masks, so its remaining high
    // byte is the alpha channel (the spelling used by Apache POI 58325_lt).
    _ => 0xFF00_0000,
  };
  let mut alpha = vec![0; width * height];
  for row in 0..height {
    let source_row = if header.is_top_down() {
      row
    } else {
      height - 1 - row
    };
    let source = &dib.bits[source_row * stride..source_row * stride + stride];
    for column in 0..width {
      let offset = column * BGRA_BYTES_PER_PIXEL;
      let value = u32::from_le_bytes([
        source[offset],
        source[offset + 1],
        source[offset + 2],
        source[offset + 3],
      ]);
      alpha[row * width + column] = bitfield_channel(value, alpha_mask);
    }
  }
  Ok(alpha)
}

fn cropped_emf_bitmap(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
  target: EmfBitmapDrawTarget,
) -> Result<Option<RasterPixels>, String> {
  let Some(image) = decode_bitmap_record_as_rgb(data, record_type, record_offset, record_size)?
  else {
    return Ok(None);
  };
  Ok(Some(
    target
      .source_rect
      .and_then(|source_rect| crop_raster_pixels(&image, source_rect))
      .unwrap_or(image),
  ))
}

#[derive(Clone, Copy, Debug)]
struct EmfMaskedBltPair {
  mask_target: EmfBitmapDrawTarget,
  source_offset: usize,
  source_type: u32,
  source_record_size: usize,
  source_target: EmfBitmapDrawTarget,
}

struct DecodedEmfMaskedBltPair {
  records: EmfMaskedBltPair,
  mask: RasterPixels,
  source: RasterPixels,
}

fn emf_masked_blt_pair(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
) -> Result<Option<EmfMaskedBltPair>, String> {
  if !matches!(record_type, EMR_BIT_BLT | EMR_STRETCH_BLT) {
    return Ok(None);
  }
  let Some(mask_target) = emf_bitmap_draw_target(data, record_offset, record_type, record_size)?
  else {
    return Ok(None);
  };
  if mask_target.raster_operation != Some(WmfTernaryRasterOperationCode::SRCAND) {
    return Ok(None);
  }

  let source_offset = record_offset + record_size;
  if source_offset + EMF_RECORD_HEADER_SIZE > data.len() {
    return Ok(None);
  }
  let source_type = read_u32(data, source_offset)?;
  if !matches!(
    source_type,
    EMR_BIT_BLT | EMR_STRETCH_BLT | EMR_STRETCH_DIBITS
  ) {
    return Ok(None);
  }
  let source_record_size = read_u32(data, source_offset + 4)? as usize;
  if source_record_size < EMF_RECORD_HEADER_SIZE || source_offset + source_record_size > data.len()
  {
    return Ok(None);
  }
  let Some(source_target) =
    emf_bitmap_draw_target(data, source_offset, source_type, source_record_size)?
  else {
    return Ok(None);
  };
  if source_target.raster_operation != Some(WmfTernaryRasterOperationCode::SRCINVERT)
    || !same_bitmap_destination(mask_target, source_target)
  {
    return Ok(None);
  }
  Ok(Some(EmfMaskedBltPair {
    mask_target,
    source_offset,
    source_type,
    source_record_size,
    source_target,
  }))
}

fn decode_emf_masked_blt_pair(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
) -> Result<Option<DecodedEmfMaskedBltPair>, String> {
  let Some(records) = emf_masked_blt_pair(data, record_offset, record_type, record_size)? else {
    return Ok(None);
  };
  let Some(mask) = cropped_emf_bitmap(
    data,
    record_offset,
    record_type,
    record_size,
    records.mask_target,
  )?
  else {
    return Ok(None);
  };
  if !is_binary_monochrome_raster(&mask) {
    return Ok(None);
  }
  let Some(source) = cropped_emf_bitmap(
    data,
    records.source_offset,
    records.source_type,
    records.source_record_size,
    records.source_target,
  )?
  else {
    return Ok(None);
  };
  if mask.width != source.width || mask.height != source.height {
    return Ok(None);
  }
  Ok(Some(DecodedEmfMaskedBltPair {
    records,
    mask,
    source,
  }))
}

fn emf_uses_binary_coverage_surface(data: &[u8]) -> Result<bool, String> {
  let Some(mut record_offset) = emf_header_record_size(data) else {
    return Ok(false);
  };
  while record_offset + EMF_RECORD_HEADER_SIZE <= data.len() {
    let record_type = read_u32(data, record_offset)?;
    let record_size = read_u32(data, record_offset + 4)? as usize;
    if record_size < EMF_RECORD_HEADER_SIZE || record_offset + record_size > data.len() {
      return Err(format!(
        "invalid EMF record at offset {record_offset}: type=0x{record_type:08x} size={record_size}"
      ));
    }
    if decode_emf_masked_blt_pair(data, record_offset, record_type, record_size)?.is_some() {
      return Ok(true);
    }
    record_offset += record_size;
    if record_type == EMR_EOF {
      break;
    }
  }
  Ok(false)
}

fn replay_masked_blt_pair(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
  state: &mut EmfVectorState,
) -> Result<Option<usize>, String> {
  let Some(pair) = decode_emf_masked_blt_pair(data, record_offset, record_type, record_size)?
  else {
    return Ok(None);
  };

  state.draw_masked_rgb_image(
    pair.records.mask_target.dest_x,
    pair.records.mask_target.dest_y,
    pair.records.mask_target.dest_width,
    pair.records.mask_target.dest_height,
    &pair.source,
    &pair.mask,
  );
  Ok(Some(pair.records.source_record_size))
}

#[derive(Clone, Debug)]
struct AlphaBlendRaster {
  pixels: RasterPixels,
  /// Per-pixel alpha for AC_SRC_ALPHA. The RGB channels remain in the DIB's
  /// premultiplied form, as required by Win32 BLENDFUNCTION playback.
  source_alpha: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct RasterPixels {
  width: usize,
  height: usize,
  rgb: Vec<u8>,
}

fn crop_alpha_blend_raster(
  image: AlphaBlendRaster,
  (x, y, width, height): (i32, i32, i32, i32),
) -> Option<AlphaBlendRaster> {
  let (x, y, width, height) = (
    usize::try_from(x).ok()?,
    usize::try_from(y).ok()?,
    usize::try_from(width).ok()?,
    usize::try_from(height).ok()?,
  );
  let right = x.checked_add(width)?;
  let bottom = y.checked_add(height)?;
  if width == 0 || height == 0 || right > image.pixels.width || bottom > image.pixels.height {
    return None;
  }
  if x == 0 && y == 0 && width == image.pixels.width && height == image.pixels.height {
    return Some(image);
  }

  let mut rgb = Vec::with_capacity(width * height * RGB_BYTES_PER_PIXEL);
  let mut source_alpha = image
    .source_alpha
    .as_ref()
    .map(|_| Vec::with_capacity(width * height));
  for row in y..bottom {
    let rgb_start = (row * image.pixels.width + x) * RGB_BYTES_PER_PIXEL;
    let rgb_end = rgb_start + width * RGB_BYTES_PER_PIXEL;
    rgb.extend_from_slice(&image.pixels.rgb[rgb_start..rgb_end]);
    if let (Some(source), Some(target)) = (&image.source_alpha, &mut source_alpha) {
      let start = row * image.pixels.width + x;
      target.extend_from_slice(&source[start..start + width]);
    }
  }
  Some(AlphaBlendRaster {
    pixels: RasterPixels { width, height, rgb },
    source_alpha,
  })
}

fn raster_color(image: &RasterPixels, x: usize, y: usize) -> EmfColor {
  let x = x.min(image.width.saturating_sub(1));
  let y = y.min(image.height.saturating_sub(1));
  let offset = (y * image.width + x) * RGB_BYTES_PER_PIXEL;
  EmfColor {
    r: image.rgb[offset],
    g: image.rgb[offset + 1],
    b: image.rgb[offset + 2],
  }
}

fn nearest_raster_index(destination: usize, destination_size: usize, source_size: usize) -> usize {
  if destination_size == 0 || source_size <= 1 {
    return 0;
  }
  // Sample at destination pixel centers, as GDI's COLORONCOLOR/nearest
  // StretchBlt mode does. Sampling from the leading edge biases duplicated
  // source columns toward the trailing side.
  let numerator = (destination as u128 * 2 + 1) * source_size as u128;
  (numerator / (destination_size as u128 * 2)).min((source_size - 1) as u128) as usize
}

fn is_binary_monochrome_raster(image: &RasterPixels) -> bool {
  image
    .rgb
    .chunks_exact(RGB_BYTES_PER_PIXEL)
    .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2] && matches!(pixel[0], 0 | u8::MAX))
}

fn is_discrete_two_color_raster(image: &RasterPixels) -> bool {
  let mut colors = [[0u8; RGB_BYTES_PER_PIXEL]; 2];
  let mut color_count = 0;
  for pixel in image.rgb.chunks_exact(RGB_BYTES_PER_PIXEL) {
    let color = [pixel[0], pixel[1], pixel[2]];
    if colors[..color_count].contains(&color) {
      continue;
    }
    if color_count == colors.len() {
      return false;
    }
    colors[color_count] = color;
    color_count += 1;
  }
  true
}

fn bilinear_raster_color(
  image: &RasterPixels,
  x: usize,
  y: usize,
  target_width: usize,
  target_height: usize,
) -> EmfColor {
  if image.width == 0 || image.height == 0 || target_width == 0 || target_height == 0 {
    return EmfColor { r: 0, g: 0, b: 0 };
  }
  let source_coordinate = |target: usize, source_extent: usize, target_extent: usize| {
    ((target as f32 + 0.5) * source_extent as f32 / target_extent as f32 - 0.5)
      .clamp(0.0, source_extent.saturating_sub(1) as f32)
  };
  let source_x = source_coordinate(x, image.width, target_width);
  let source_y = source_coordinate(y, image.height, target_height);
  let x0 = source_x.floor() as usize;
  let y0 = source_y.floor() as usize;
  let x1 = (x0 + 1).min(image.width - 1);
  let y1 = (y0 + 1).min(image.height - 1);
  let fraction_x = source_x - x0 as f32;
  let fraction_y = source_y - y0 as f32;
  let top_left = raster_color(image, x0, y0);
  let top_right = raster_color(image, x1, y0);
  let bottom_left = raster_color(image, x0, y1);
  let bottom_right = raster_color(image, x1, y1);
  let channel = |top_left: u8, top_right: u8, bottom_left: u8, bottom_right: u8| {
    let top = f32::from(top_left) + (f32::from(top_right) - f32::from(top_left)) * fraction_x;
    let bottom =
      f32::from(bottom_left) + (f32::from(bottom_right) - f32::from(bottom_left)) * fraction_x;
    (top + (bottom - top) * fraction_y)
      .round()
      .clamp(0.0, f32::from(u8::MAX)) as u8
  };
  EmfColor {
    r: channel(top_left.r, top_right.r, bottom_left.r, bottom_right.r),
    g: channel(top_left.g, top_right.g, bottom_left.g, bottom_right.g),
    b: channel(top_left.b, top_right.b, bottom_left.b, bottom_right.b),
  }
}

fn bilinear_raster_plane_value(
  plane: &[u8],
  source_width: usize,
  source_height: usize,
  x: usize,
  y: usize,
  target_width: usize,
  target_height: usize,
) -> u8 {
  if source_width == 0
    || source_height == 0
    || target_width == 0
    || target_height == 0
    || plane.len() < source_width.saturating_mul(source_height)
  {
    return 0;
  }
  let source_coordinate = |target: usize, source_extent: usize, target_extent: usize| {
    ((target as f32 + 0.5) * source_extent as f32 / target_extent as f32 - 0.5)
      .clamp(0.0, source_extent.saturating_sub(1) as f32)
  };
  let source_x = source_coordinate(x, source_width, target_width);
  let source_y = source_coordinate(y, source_height, target_height);
  let x0 = source_x.floor() as usize;
  let y0 = source_y.floor() as usize;
  let x1 = (x0 + 1).min(source_width - 1);
  let y1 = (y0 + 1).min(source_height - 1);
  let fraction_x = source_x - x0 as f32;
  let fraction_y = source_y - y0 as f32;
  let top_left = plane[y0 * source_width + x0];
  let top_right = plane[y0 * source_width + x1];
  let bottom_left = plane[y1 * source_width + x0];
  let bottom_right = plane[y1 * source_width + x1];
  let top = f32::from(top_left) + (f32::from(top_right) - f32::from(top_left)) * fraction_x;
  let bottom =
    f32::from(bottom_left) + (f32::from(bottom_right) - f32::from(bottom_left)) * fraction_x;
  (top + (bottom - top) * fraction_y)
    .round()
    .clamp(0.0, f32::from(u8::MAX)) as u8
}

fn gdi_alpha_blend_color(
  destination: EmfColor,
  source: EmfColor,
  source_alpha: Option<u8>,
  source_constant_alpha: u8,
) -> EmfColor {
  let blend = |destination: u8, source: u8| match source_alpha {
    Some(source_alpha) => {
      // AC_SRC_ALPHA stores premultiplied source channels. SrcConstantAlpha
      // scales both those channels and the per-pixel alpha before SourceOver;
      // multiplying the color by the effective alpha again would darken
      // translucent artwork a second time.
      let constant = u32::from(source_constant_alpha);
      let source = (u32::from(source) * constant + 127) / 255;
      let alpha = (u32::from(source_alpha) * constant + 127) / 255;
      (source + (u32::from(destination) * (255 - alpha) + 127) / 255).min(255) as u8
    }
    None => {
      let alpha = u32::from(source_constant_alpha);
      ((u32::from(source) * alpha + u32::from(destination) * (255 - alpha) + 127) / 255) as u8
    }
  };
  EmfColor {
    r: blend(destination.r, source.r),
    g: blend(destination.g, source.g),
    b: blend(destination.b, source.b),
  }
}

/// Samples the independently filtered color branch of a GDI+ metafile blit.
///
/// `Graphics::DrawImage(Metafile, destination)` maps the first destination
/// sample to the first source sample and advances by `(source - 1) / target`.
/// This differs from the half-pixel convention used by ordinary decoded
/// images. A 32-sample 0,8,..,248 ramp stretched to 66 samples therefore
/// begins `0,4,8,11,15` and ends at 244 in Windows GDI+ playback.
fn gdi_plus_bilinear_raster_color(
  image: &RasterPixels,
  x: usize,
  y: usize,
  target_width: usize,
  target_height: usize,
) -> EmfColor {
  if image.width == 0 || image.height == 0 || target_width == 0 || target_height == 0 {
    return EmfColor { r: 0, g: 0, b: 0 };
  }
  let source_coordinate = |target: usize, source_extent: usize, target_extent: usize| {
    target as f32 * source_extent.saturating_sub(1) as f32 / target_extent as f32
  };
  let source_x = source_coordinate(x, image.width, target_width);
  let source_y = source_coordinate(y, image.height, target_height);
  let x0 = source_x.floor() as usize;
  let y0 = source_y.floor() as usize;
  let x1 = (x0 + 1).min(image.width - 1);
  let y1 = (y0 + 1).min(image.height - 1);
  let fraction_x = source_x - x0 as f32;
  let fraction_y = source_y - y0 as f32;
  let top_left = raster_color(image, x0, y0);
  let top_right = raster_color(image, x1, y0);
  let bottom_left = raster_color(image, x0, y1);
  let bottom_right = raster_color(image, x1, y1);
  let channel = |top_left: u8, top_right: u8, bottom_left: u8, bottom_right: u8| {
    let top = f32::from(top_left) + (f32::from(top_right) - f32::from(top_left)) * fraction_x;
    let bottom =
      f32::from(bottom_left) + (f32::from(bottom_right) - f32::from(bottom_left)) * fraction_x;
    (top + (bottom - top) * fraction_y)
      .round()
      .clamp(0.0, f32::from(u8::MAX)) as u8
  };
  EmfColor {
    r: channel(top_left.r, top_right.r, bottom_left.r, bottom_right.r),
    g: channel(top_left.g, top_right.g, bottom_left.g, bottom_right.g),
    b: channel(top_left.b, top_right.b, bottom_left.b, bottom_right.b),
  }
}

fn checkerboard_average_color(image: &RasterPixels) -> Option<EmfColor> {
  if image.width < 2 || image.height < 2 {
    return None;
  }
  let color_at = |x: usize, y: usize| {
    let offset = (y * image.width + x) * RGB_BYTES_PER_PIXEL;
    EmfColor {
      r: image.rgb[offset],
      g: image.rgb[offset + 1],
      b: image.rgb[offset + 2],
    }
  };
  let first = color_at(0, 0);
  let second = color_at(1, 0);
  if first == second {
    return None;
  }
  for y in 0..image.height {
    for x in 0..image.width {
      let expected = if (x + y).is_multiple_of(2) {
        first
      } else {
        second
      };
      if color_at(x, y) != expected {
        return None;
      }
    }
  }
  Some(EmfColor {
    r: ((u16::from(first.r) + u16::from(second.r)) / 2) as u8,
    g: ((u16::from(first.g) + u16::from(second.g)) / 2) as u8,
    b: ((u16::from(first.b) + u16::from(second.b)) / 2) as u8,
  })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EmfBitmapDrawTarget {
  dest_x: i32,
  dest_y: i32,
  dest_width: i32,
  dest_height: i32,
  raster_operation: Option<WmfTernaryRasterOperationCode>,
  source_rect: Option<(i32, i32, i32, i32)>,
}

fn same_bitmap_destination(first: EmfBitmapDrawTarget, second: EmfBitmapDrawTarget) -> bool {
  first.dest_x == second.dest_x
    && first.dest_y == second.dest_y
    && first.dest_width == second.dest_width
    && first.dest_height == second.dest_height
}

#[derive(Clone, Debug)]
struct WmfSavedState {
  window_org_x: i32,
  window_org_y: i32,
  window_ext_x: i32,
  window_ext_y: i32,
  viewport_org_x: i32,
  viewport_org_y: i32,
  viewport_ext_x: i32,
  viewport_ext_y: i32,
  current_brush: Option<EmfColor>,
  current_pen: Option<EmfPen>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  binary_raster_operation: WmfBinaryRasterOperation,
  background_color: EmfColor,
  current_pattern_brush: Option<WmfPatternBrush>,
  current_solid_brush: bool,
  current_font: WmfTextFont,
  text_alignment: WmfTextAlignmentModeFlags,
}

#[derive(Clone, Debug)]
struct WmfPatternBrush {
  image: RasterPixels,
  use_dc_colors: bool,
  filtered_color: Option<EmfColor>,
}

#[derive(Clone, Debug)]
enum WmfRenderObject {
  Pen(Option<EmfPen>),
  Brush {
    color: Option<EmfColor>,
    solid: bool,
  },
  PatternBrush(WmfPatternBrush),
  Font(WmfTextFont),
  Unsupported,
}

struct WmfRenderState {
  canvas: EmfVectorState,
  objects: Vec<Option<WmfRenderObject>>,
  current_pos: EmfPoint,
  text_color: EmfColor,
  background_color: EmfColor,
  current_pattern_brush: Option<WmfPatternBrush>,
  current_solid_brush: bool,
  current_font: WmfTextFont,
  text_alignment: WmfTextAlignmentModeFlags,
  saved: Vec<WmfSavedState>,
}

impl WmfRenderState {
  fn new(
    metafile: &WmfMetafileRef<'_>,
    options: RenderOptions,
    text_surface: GdiTextSurface,
  ) -> Result<Self, String> {
    let (window_org_x, window_org_y, window_ext_x, window_ext_y) =
      wmf_initial_window(metafile, options.wmf_external_header);
    let natural_width = window_ext_x.unsigned_abs().max(1) as usize;
    let natural_height = window_ext_y.unsigned_abs().max(1) as usize;
    let (width, height) = options.resolved_canvas_size(natural_width, natural_height);
    let output_scale_x = width as f32 / natural_width as f32;
    let output_scale_y = height as f32 / natural_height as f32;
    let object_count = metafile.header.number_of_objects as usize;
    let background_color = options.background_color.unwrap_or([255; 3]);
    let mut rgb = vec![0; width * height * RGB_BYTES_PER_PIXEL];
    for pixel in rgb.chunks_exact_mut(RGB_BYTES_PER_PIXEL) {
      pixel.copy_from_slice(&background_color);
    }

    Ok(Self {
      canvas: EmfVectorState {
        width,
        height,
        natural_width,
        natural_height,
        playback_origin_x: 0.0,
        playback_origin_y: 0.0,
        playback_scale_x: 1.0,
        playback_scale_y: 1.0,
        output_scale_x,
        output_scale_y,
        // WMF owns its mapping state in WmfRenderState. Keep this shared
        // canvas on the variable-extent path so META_SETWINDOWEXT and the
        // caller-supplied viewport retain their existing mapping.
        map_mode: EmrMapMode::Anisotropic,
        window_org_x,
        window_org_y,
        window_ext_x: nonzero_mapping_extent(window_ext_x),
        window_ext_y: nonzero_mapping_extent(window_ext_y),
        viewport_org_x: 0,
        viewport_org_y: 0,
        viewport_ext_x: natural_width as i32,
        viewport_ext_y: natural_height as i32,
        world_transform: EmfTransform::identity(),
        emf_plus_page_unit: EmfPlusUnitType::Pixel,
        emf_plus_page_scale: 1.0,
        emf_plus_logical_dpi_x: 96.0,
        emf_plus_logical_dpi_y: 96.0,
        emf_plus_video_display: true,
        brush_colors: std::collections::HashMap::new(),
        solid_brushes: std::collections::HashSet::new(),
        pens: std::collections::HashMap::new(),
        fonts: std::collections::HashMap::new(),
        current_brush: Some(EmfColor {
          r: 255,
          g: 255,
          b: 255,
        }),
        current_brush_is_solid: true,
        current_pen: Some(EmfPen {
          color: EmfColor { r: 0, g: 0, b: 0 },
          alpha: u8::MAX,
          width: 1,
          width_space: EmfPenWidthSpace::Device,
        }),
        current_font: None,
        current_pos: EmfPoint { x: 0, y: 0 },
        text_color: EmfColor { r: 0, g: 0, b: 0 },
        binary_raster_operation: WmfBinaryRasterOperation::CopyPen,
        text_alignment: WmfTextAlignmentModeFlags::empty(),
        clip_rect: None,
        clip_mask: None,
        saved_states: Vec::new(),
        emf_plus_saved_states: Vec::new(),
        emf_plus_containers: Vec::new(),
        emf_plus_objects: Vec::new(),
        emf_plus_object_assembler: EmfPlusObjectAssembler::default(),
        font_cache: RenderFontCache::load(),
        text_surface,
        suppress_text: options.suppress_text,
        rgb,
      },
      objects: vec![None; object_count],
      current_pos: EmfPoint { x: 0, y: 0 },
      text_color: EmfColor { r: 0, g: 0, b: 0 },
      background_color: EmfColor {
        r: 255,
        g: 255,
        b: 255,
      },
      current_pattern_brush: None,
      current_solid_brush: false,
      current_font: WmfTextFont {
        height: 12,
        family: None,
        char_set: 0,
        weight: 400,
        italic: false,
        quality: crate::wmf::WmfFontQuality::Default.raw(),
      },
      text_alignment: WmfTextAlignmentModeFlags::empty(),
      saved: Vec::new(),
    })
  }

  fn insert_object(&mut self, object: WmfRenderObject) {
    if let Some(slot) = self.objects.iter_mut().find(|slot| slot.is_none()) {
      *slot = Some(object);
    } else {
      self.objects.push(Some(object));
    }
  }

  fn save_dc(&mut self) {
    self.saved.push(WmfSavedState {
      window_org_x: self.canvas.window_org_x,
      window_org_y: self.canvas.window_org_y,
      window_ext_x: self.canvas.window_ext_x,
      window_ext_y: self.canvas.window_ext_y,
      viewport_org_x: self.canvas.viewport_org_x,
      viewport_org_y: self.canvas.viewport_org_y,
      viewport_ext_x: self.canvas.viewport_ext_x,
      viewport_ext_y: self.canvas.viewport_ext_y,
      current_brush: self.canvas.current_brush,
      current_pen: self.canvas.current_pen,
      current_pos: self.current_pos,
      text_color: self.text_color,
      binary_raster_operation: self.canvas.binary_raster_operation,
      background_color: self.background_color,
      current_pattern_brush: self.current_pattern_brush.clone(),
      current_solid_brush: self.current_solid_brush,
      current_font: self.current_font.clone(),
      text_alignment: self.text_alignment,
    });
  }

  fn restore_dc(&mut self) {
    let Some(saved) = self.saved.pop() else {
      return;
    };
    self.canvas.window_org_x = saved.window_org_x;
    self.canvas.window_org_y = saved.window_org_y;
    self.canvas.window_ext_x = saved.window_ext_x;
    self.canvas.window_ext_y = saved.window_ext_y;
    self.canvas.viewport_org_x = saved.viewport_org_x;
    self.canvas.viewport_org_y = saved.viewport_org_y;
    self.canvas.viewport_ext_x = saved.viewport_ext_x;
    self.canvas.viewport_ext_y = saved.viewport_ext_y;
    self.canvas.current_brush = saved.current_brush;
    self.canvas.current_pen = saved.current_pen;
    self.current_pos = saved.current_pos;
    self.text_color = saved.text_color;
    self.canvas.binary_raster_operation = saved.binary_raster_operation;
    self.background_color = saved.background_color;
    self.current_pattern_brush = saved.current_pattern_brush;
    self.current_solid_brush = saved.current_solid_brush;
    self.current_font = saved.current_font;
    self.text_alignment = saved.text_alignment;
  }

  fn select_object(&mut self, index: u16) {
    let Some(Some(object)) = self.objects.get(index as usize).cloned() else {
      return;
    };
    match object {
      WmfRenderObject::Pen(pen) => self.canvas.current_pen = pen,
      WmfRenderObject::Brush { color, solid } => {
        self.canvas.current_brush = color;
        self.current_pattern_brush = None;
        self.current_solid_brush = solid;
      }
      WmfRenderObject::PatternBrush(pattern) => {
        self.current_pattern_brush = Some(pattern);
        self.current_solid_brush = false;
      }
      WmfRenderObject::Font(font) => self.current_font = font,
      WmfRenderObject::Unsupported => {}
    }
  }

  fn delete_object(&mut self, index: u16) {
    if let Some(slot) = self.objects.get_mut(index as usize) {
      *slot = None;
    }
  }

  fn text_reference_point(&self, x: i16, y: i16) -> EmfPoint {
    if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::UPDATE_CP)
    {
      self.current_pos
    } else {
      EmfPoint {
        x: i32::from(x),
        y: i32::from(y),
      }
    }
  }

  fn text_origin(&self, x: i16, y: i16, logical_width: Option<i32>) -> EmfPoint {
    let mut reference = self.text_reference_point(x, y);
    if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::CENTER)
    {
      reference.x = reference
        .x
        .saturating_sub(logical_width.unwrap_or_default() / 2);
    } else if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::RIGHT)
    {
      reference.x = reference
        .x
        .saturating_sub(logical_width.unwrap_or_default());
    }
    reference
  }

  fn text_baseline_y(&self, reference_y: i32) -> i32 {
    if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::BASELINE)
      || self
        .text_alignment
        .contains(WmfTextAlignmentModeFlags::BOTTOM)
    {
      reference_y
    } else {
      // [MS-WMF] 2.1.2.3 defines the all-zero vertical mode as TA_TOP.
      // Our outline painter takes a baseline, so advance by the logical
      // character-cell height before applying the device mapping.
      reference_y.saturating_add(self.current_font.height.unsigned_abs() as i32)
    }
  }

  fn update_current_position_after_text(
    &mut self,
    text_origin: EmfPoint,
    logical_width: Option<i32>,
  ) {
    if self
      .text_alignment
      .contains(WmfTextAlignmentModeFlags::UPDATE_CP)
      && let Some(logical_width) = logical_width
    {
      self.current_pos.x = text_origin.x.saturating_add(logical_width);
      self.current_pos.y = text_origin.y;
    }
  }

  fn fill_pattern_rect(
    &mut self,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    rop: WmfTernaryRasterOperationCode,
  ) -> bool {
    let Some(pattern) = self.current_pattern_brush.as_ref() else {
      return false;
    };
    let (mapped_left, mapped_top) = self.canvas.map_point(EmfPoint { x: left, y: top });
    let (mapped_right, mapped_bottom) = self.canvas.map_point(EmfPoint {
      x: right,
      y: bottom,
    });
    let left = mapped_left.min(mapped_right).round().max(0.0) as i32;
    let top = mapped_top.min(mapped_bottom).round().max(0.0) as i32;
    let right = mapped_left
      .max(mapped_right)
      .round()
      .min(self.canvas.width as f32) as i32;
    let bottom = mapped_top
      .max(mapped_bottom)
      .round()
      .min(self.canvas.height as f32) as i32;
    for y in top..bottom {
      for x in left..right {
        let pattern_x = x.rem_euclid(pattern.image.width as i32) as usize;
        let pattern_y = y.rem_euclid(pattern.image.height as i32) as usize;
        let offset = (pattern_y * pattern.image.width + pattern_x) * RGB_BYTES_PER_PIXEL;
        let stored = pattern.filtered_color.unwrap_or(EmfColor {
          r: pattern.image.rgb[offset],
          g: pattern.image.rgb[offset + 1],
          b: pattern.image.rgb[offset + 2],
        });
        let brush = if pattern.use_dc_colors {
          if u16::from(stored.r) + u16::from(stored.g) + u16::from(stored.b) < 3 * 128 {
            self.text_color
          } else {
            self.background_color
          }
        } else {
          stored
        };
        if let Some(color) = self
          .canvas
          .apply_raster_op_with_pattern(x, y, brush, brush, rop)
        {
          self.canvas.set_pixel(x, y, color);
        }
      }
    }
    true
  }
}

fn decode_wmf_as_raster(
  data: &[u8],
  options: RenderOptions,
  text_surface: GdiTextSurface,
) -> Result<Option<DecodedMetafile>, String> {
  if !crate::wmf::looks_like_wmf(data) {
    return Ok(None);
  }

  let metafile = WmfMetafileRef::from_bytes(data).map_err(|err| err.to_string())?;
  let mut state = WmfRenderState::new(&metafile, options, text_surface)?;

  let mut records = metafile.records().peekable();
  while let Some(record) = records.next() {
    // Compatibility-mode parsing preserves producer-specific and malformed
    // records so later valid drawing commands remain usable. Rendering must
    // follow the same recovery rule: one unsupported device escape must not
    // discard the entire preview that was already replayed.
    let Ok(parsed) = record.parse_data() else {
      continue;
    };
    match parsed {
      WmfRecordData::Eof(_) => break,
      WmfRecordData::SaveDc => state.save_dc(),
      WmfRecordData::RestoreDc(_) => state.restore_dc(),
      WmfRecordData::SetWindowOrg(value) => {
        state.canvas.window_org_x = i32::from(value.x);
        state.canvas.window_org_y = i32::from(value.y);
      }
      WmfRecordData::SetWindowExt(value) => {
        state.canvas.window_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.canvas.window_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::SetViewportOrg(value) => {
        state.canvas.viewport_org_x = i32::from(value.x);
        state.canvas.viewport_org_y = i32::from(value.y);
      }
      WmfRecordData::SetViewportExt(value) => {
        state.canvas.viewport_ext_x = nonzero_mapping_extent(i32::from(value.x));
        state.canvas.viewport_ext_y = nonzero_mapping_extent(i32::from(value.y));
      }
      WmfRecordData::IntersectClipRect(value) => {
        state.canvas.set_clip_rect_logical(
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        );
      }
      WmfRecordData::ExcludeClipRect(_) => {}
      WmfRecordData::SetTextColor(value) => {
        state.text_color = color_ref_to_emf(value.color);
      }
      WmfRecordData::SetRop2(value) => {
        if let Some(operation) = value.binary_raster_operation_kind() {
          state.canvas.binary_raster_operation = operation;
        }
      }
      WmfRecordData::SetTextAlign(value) => {
        state.text_alignment = value.text_alignment_flags();
      }
      WmfRecordData::SetBkColor(value) => {
        state.background_color = color_ref_to_emf(value.color);
      }
      WmfRecordData::OffsetWindowOrg(value) => {
        state.canvas.window_org_x += i32::from(value.x);
        state.canvas.window_org_y += i32::from(value.y);
      }
      WmfRecordData::OffsetViewportOrg(value) => {
        state.canvas.viewport_org_x += i32::from(value.x);
        state.canvas.viewport_org_y += i32::from(value.y);
      }
      WmfRecordData::CreatePenIndirect(value) => {
        let line_style = WmfPenLineStyle::from_raw(value.pen.pen_line_style_raw());
        let pen = if line_style == Some(WmfPenLineStyle::Null) {
          None
        } else {
          let (width, width_space) = wmf_pen_width(value.pen.width.x);
          let pen = EmfPen {
            color: color_ref_to_emf(value.pen.color_ref),
            alpha: u8::MAX,
            width,
            // [MS-WMF] 2.2.1.4 ignores width.y; 3.1.4.2 requires a
            // nonzero logical width to be realized as an x-scalar. Width 0
            // is the mapping-mode-independent one-device-pixel hairline.
            width_space,
          };
          // A WMF graphics object is realized under the mapping active when
          // META_CREATEPENINDIRECT is played. Later mapping records must not
          // retroactively resize the stored pen.
          Some(state.canvas.resolve_pen(pen))
        };
        state.insert_object(WmfRenderObject::Pen(pen));
      }
      WmfRecordData::CreateBrushIndirect(value) => {
        let style = value.brush_style_kind();
        let color = match style {
          Some(WmfBrushStyle::Null) => None,
          _ => Some(color_ref_to_emf(value.color_ref)),
        };
        state.insert_object(WmfRenderObject::Brush {
          color,
          solid: style == Some(WmfBrushStyle::Solid),
        });
      }
      WmfRecordData::CreateFontIndirect(value) => {
        state.insert_object(WmfRenderObject::Font(wmf_text_font(&value)));
      }
      WmfRecordData::CreatePatternBrush(value) => {
        let pattern = value
          .bitmap16()
          .ok()
          .and_then(|bitmap| bitmap.to_bytes().ok())
          .and_then(|bytes| bitmap16_to_rgb(&bytes).ok().flatten())
          .map(|image| {
            let filtered_color = options
              .filter_high_frequency_pattern_brushes
              .then(|| checkerboard_average_color(&image))
              .flatten();
            WmfPatternBrush {
              image,
              use_dc_colors: value.bitmap.bits_pixel == 1,
              filtered_color,
            }
          });
        state.insert_object(
          pattern
            .map(WmfRenderObject::PatternBrush)
            .unwrap_or(WmfRenderObject::Unsupported),
        );
      }
      WmfRecordData::DibCreatePatternBrush(value) => {
        let pattern = value
          .color_usage_kind()
          .and_then(|usage| {
            packed_dib_to_rgb_with_palette_override(
              &value.target,
              usage,
              options.monochrome_dib_palette_override,
            )
            .ok()
            .flatten()
          })
          .map(|image| {
            let filtered_color = options
              .filter_high_frequency_pattern_brushes
              .then(|| checkerboard_average_color(&image))
              .flatten();
            WmfPatternBrush {
              image,
              // DIB pattern brushes retain their color table on a color
              // playback surface. The DC text/background substitution applies
              // only when GDI realizes the brush into a monochrome target.
              use_dc_colors: false,
              filtered_color,
            }
          });
        state.insert_object(
          pattern
            .map(WmfRenderObject::PatternBrush)
            .unwrap_or(WmfRenderObject::Unsupported),
        );
      }
      WmfRecordData::CreatePalette(_) | WmfRecordData::CreateRegion(_) => {
        state.insert_object(WmfRenderObject::Unsupported);
      }
      WmfRecordData::SelectObject(value) => state.select_object(value.index),
      WmfRecordData::DeleteObject(value) => state.delete_object(value.index),
      WmfRecordData::MoveTo(value) => {
        state.current_pos = EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        };
      }
      WmfRecordData::LineTo(value) => {
        let next = EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        };
        state
          .canvas
          .draw_polyline(&[state.current_pos, next], false);
        state.current_pos = next;
      }
      WmfRecordData::SetPixel(value) => {
        let (x, y) = state.canvas.map_point(EmfPoint {
          x: i32::from(value.x),
          y: i32::from(value.y),
        });
        state.canvas.set_pixel(
          x.round() as i32,
          y.round() as i32,
          color_ref_to_emf(value.color),
        );
      }
      WmfRecordData::Polygon(value) => {
        let points = value
          .points
          .iter()
          .map(|point| EmfPoint {
            x: i32::from(point.x),
            y: i32::from(point.y),
          })
          .collect::<Vec<_>>();
        state.canvas.fill_polygon(&points);
        state.canvas.draw_polyline(&points, true);
      }
      WmfRecordData::Polyline(value) => {
        let points = value
          .points
          .iter()
          .map(|point| EmfPoint {
            x: i32::from(point.x),
            y: i32::from(point.y),
          })
          .collect::<Vec<_>>();
        state.canvas.draw_polyline(&points, false);
      }
      WmfRecordData::PolyPolygon(value) => {
        let mut cursor = 0usize;
        for count in value.points_per_polygon {
          let end = cursor
            .saturating_add(count as usize)
            .min(value.points.len());
          let points = value.points[cursor..end]
            .iter()
            .map(|point| EmfPoint {
              x: i32::from(point.x),
              y: i32::from(point.y),
            })
            .collect::<Vec<_>>();
          state.canvas.fill_polygon(&points);
          state.canvas.draw_polyline(&points, true);
          cursor = end;
        }
      }
      WmfRecordData::Rectangle(value) => state.canvas.fill_rect(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::RoundRect(value) => state.canvas.fill_rect(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::Ellipse(value) => state.canvas.fill_ellipse(
        i32::from(value.left),
        i32::from(value.top),
        i32::from(value.right),
        i32::from(value.bottom),
      ),
      WmfRecordData::Arc(value) => state.canvas.fill_arc_segment(
        (
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        ),
        angle_from_arc_point(value, value.x_radial_1, value.y_radial_1),
        sweep_from_arc_points(value),
        false,
      ),
      WmfRecordData::Chord(value) | WmfRecordData::Pie(value) => state.canvas.fill_arc_segment(
        (
          i32::from(value.left),
          i32::from(value.top),
          i32::from(value.right),
          i32::from(value.bottom),
        ),
        angle_from_arc_point(value, value.x_radial_1, value.y_radial_1),
        sweep_from_arc_points(value),
        true,
      ),
      WmfRecordData::TextOut(value) => {
        let text = decode_wmf_text(&value.string, state.current_font.char_set);
        let origin = state.text_origin(value.x_start, value.y_start, None);
        let baseline_y = state.text_baseline_y(origin.y);
        state.canvas.draw_text_with_font(
          origin.x,
          baseline_y,
          &text,
          state.text_color,
          &state.current_font,
        );
        state.update_current_position_after_text(origin, None);
      }
      WmfRecordData::ExtTextOut(value) => {
        if let Some(rectangle) = value.rectangle
          && value.options.contains(WmfExtTextOutOptions::OPAQUE)
        {
          // [MS-WMF] 2.1.2.2: ETO_OPAQUE fills the application-defined
          // rectangle with the playback DC's current background color.
          state.canvas.fill_solid_rect(
            i32::from(rectangle.left),
            i32::from(rectangle.top),
            i32::from(rectangle.right),
            i32::from(rectangle.bottom),
            state.background_color,
          );
        }

        let text = decode_wmf_text(&value.string, state.current_font.char_set);
        let logical_width = (!value.dx.is_empty()).then(|| {
          value.dx.iter().fold(0i32, |total, advance| {
            total.saturating_add(i32::from(*advance))
          })
        });
        let origin = state.text_origin(value.x, value.y, logical_width);
        let baseline_y = state.text_baseline_y(origin.y);
        let saved_clip = value
          .rectangle
          .filter(|_| value.options.contains(WmfExtTextOutOptions::CLIPPED))
          .map(|rectangle| {
            let saved = (state.canvas.clip_rect, state.canvas.clip_mask.clone());
            state.canvas.set_clip_rect_device(
              {
                let (left, top) = state.canvas.map_point(EmfPoint {
                  x: i32::from(rectangle.left),
                  y: i32::from(rectangle.top),
                });
                let (right, bottom) = state.canvas.map_point(EmfPoint {
                  x: i32::from(rectangle.right),
                  y: i32::from(rectangle.bottom),
                });
                (
                  left.min(right).floor() as i32,
                  top.min(bottom).floor() as i32,
                  left.max(right).ceil() as i32,
                  top.max(bottom).ceil() as i32,
                )
              },
              1,
            );
            saved
          });
        state.canvas.draw_wmf_text(
          origin.x,
          baseline_y,
          &text,
          state.text_color,
          &state.current_font,
          (!value.dx.is_empty()).then_some(value.dx.as_slice()),
        );
        state.update_current_position_after_text(origin, logical_width);
        if let Some((clip_rect, clip_mask)) = saved_clip {
          state.canvas.clip_rect = clip_rect;
          state.canvas.clip_mask = clip_mask;
        }
      }
      WmfRecordData::PatBlt(value) => {
        let left = i32::from(value.x_left);
        let top = i32::from(value.y_left);
        let right = left + i32::from(value.width);
        let bottom = top + i32::from(value.height);
        let rop = value.raster_operation_code();
        let lifted_solid_rect = options.suppress_solid_pattern_rects
          && rop == WmfTernaryRasterOperationCode::PATCOPY
          && state.current_pattern_brush.is_none()
          && state.current_solid_brush
          && state.canvas.current_brush.is_some();
        if !lifted_solid_rect && !state.fill_pattern_rect(left, top, right, bottom, rop) {
          state
            .canvas
            .fill_rect_with_rop(left, top, right, bottom, rop);
        }
      }
      WmfRecordData::StretchDib(value) => {
        if let Some(color_usage) = value.color_usage_kind()
          && let Some(image) = packed_dib_to_rgb(&value.dib, color_usage)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::SetDibToDev(value) => {
        if let Some(color_usage) = value.color_usage_kind()
          && let Some(image) = packed_dib_to_rgb(&value.dib, color_usage)?
        {
          state.canvas.draw_rgb_image(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
          );
        }
      }
      WmfRecordData::DibBitBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Some(image) = packed_dib_to_rgb(bytes, DibColorUsage::RgbColors)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::DibStretchBlt(value) => {
        if options.suppress_bitmap_layers {
          let next = records
            .peek()
            .copied()
            .and_then(|record| record.parse_data().ok());
          if let Some(WmfRecordData::DibStretchBlt(second)) = next
            && wmf_masked_bitmap_pair(&value, &second).is_some()
          {
            records.next();
            continue;
          }
          if value.raster_operation_code() == WmfTernaryRasterOperationCode::SRCCOPY
            && full_wmf_dib_source(&value).is_some()
          {
            continue;
          }
        }
        if let Some(bytes) = value.target.source_bytes()
          && let Some(image) = packed_dib_to_rgb(bytes, DibColorUsage::RgbColors)?
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::BitBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Ok(Some(image)) = bitmap16_to_rgb(bytes)
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.width),
            i32::from(value.height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::StretchBlt(value) => {
        if let Some(bytes) = value.target.source_bytes()
          && let Ok(Some(image)) = bitmap16_to_rgb(bytes)
        {
          state.canvas.draw_rgb_image_with_rop(
            i32::from(value.x_dest),
            i32::from(value.y_dest),
            i32::from(value.dest_width),
            i32::from(value.dest_height),
            &image,
            value.raster_operation_code(),
          );
        }
      }
      WmfRecordData::Escape(value) => {
        if let Ok(WmfEscapeData::EnhancedMetafile {
          enhanced_metafile_data,
          ..
        }) = value.typed_data()
          && let Some(raster) = decode_emf_as_raster(
            enhanced_metafile_data,
            options,
            false,
            state.canvas.text_surface,
          )?
          && let Some(image) = decoded_raster_to_rgb(&raster)?
        {
          state.canvas.draw_rgb_image(
            state.canvas.window_org_x,
            state.canvas.window_org_y,
            state.canvas.window_ext_x,
            state.canvas.window_ext_y,
            &image,
          );
        }
      }
      _ => {}
    }
  }

  Ok(Some(DecodedMetafile {
    data: rgb_to_png(
      &state.canvas.rgb,
      state.canvas.width as u32,
      state.canvas.height as u32,
    )?,
    content_type: "image/png",
  }))
}

fn wmf_external_canvas_size(
  metafile: &WmfMetafileRef<'_>,
  external_header: Option<WmfExternalHeader>,
) -> Option<(i32, i32)> {
  if metafile.placeable_header.is_some() {
    return None;
  }
  let external_header = external_header?;
  if external_header.width_hundredths_mm == 0 || external_header.height_hundredths_mm == 0 {
    return None;
  }

  // SetWinMetaFileBits uses both METAFILEPICT and the reference DC's
  // resolution. The resulting viewport extent is a pixel count; rclBounds is
  // then reported with an inclusive last *coordinate* (for example, an extent
  // of 280 has a right bound of 279). Do not turn that coordinate convention
  // into an extra viewport pixel. A Win32 conversion of the 5080x3016-mm100
  // ActiveX counterexample at 140 DPI yields viewport extents 280x166 and
  // bounds 0,0,279,165.
  const HUNDREDTHS_MM_PER_INCH: u64 = 2_540;
  let axis = |hundredths_mm: u32, dpi: u32| {
    if dpi == 0 {
      return None;
    }
    let rounded_pixels = (u64::from(hundredths_mm) * u64::from(dpi) + HUNDREDTHS_MM_PER_INCH / 2)
      / HUNDREDTHS_MM_PER_INCH;
    i32::try_from(rounded_pixels.min(i32::MAX as u64)).ok()
  };
  Some((
    axis(
      external_header.width_hundredths_mm,
      external_header.reference_device_dpi_x,
    )?
    .max(1),
    axis(
      external_header.height_hundredths_mm,
      external_header.reference_device_dpi_y,
    )?
    .max(1),
  ))
}

fn wmf_initial_window(
  metafile: &WmfMetafileRef<'_>,
  external_header: Option<WmfExternalHeader>,
) -> (i32, i32, i32, i32) {
  if let Some(placeable) = &metafile.placeable_header {
    return (
      i32::from(placeable.left),
      i32::from(placeable.top),
      placeable.bounding_box_width().abs().max(1),
      placeable.bounding_box_height().abs().max(1),
    );
  }

  if let Some((width, height)) = wmf_external_canvas_size(metafile, external_header) {
    return (0, 0, width, height);
  }

  let mut org_x = 0;
  let mut org_y = 0;
  let mut ext_x = DEFAULT_RENDER_WIDTH as i32;
  let mut ext_y = DEFAULT_RENDER_HEIGHT as i32;
  for record in metafile.records() {
    match record.parse_data() {
      Ok(WmfRecordData::SetWindowOrg(value)) => {
        org_x = i32::from(value.x);
        org_y = i32::from(value.y);
      }
      Ok(WmfRecordData::SetWindowExt(value)) => {
        ext_x = nonzero_mapping_extent(i32::from(value.x));
        ext_y = nonzero_mapping_extent(i32::from(value.y));
        break;
      }
      Ok(WmfRecordData::Eof(_)) => break,
      _ => {}
    }
  }
  (org_x, org_y, ext_x, ext_y)
}

fn emf_bitmap_draw_target(
  data: &[u8],
  record_offset: usize,
  record_type: u32,
  record_size: usize,
) -> Result<Option<EmfBitmapDrawTarget>, String> {
  let min_size = match record_type {
    EMR_BIT_BLT => EMR_BLT_BITS_SIZE_OFFSET + 4,
    EMR_STRETCH_BLT => EMR_STRETCH_BLT_SOURCE_HEIGHT_OFFSET + 4,
    EMR_SET_DIBITS_TO_DEVICE => EMR_BITMAP_BITS_SIZE_OFFSET + 4,
    EMR_STRETCH_DIBITS => EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET + 4,
    _ => return Ok(None),
  };
  if record_size < min_size {
    return Ok(None);
  }

  let dest_x = read_i32(data, record_offset + EMR_BITMAP_DEST_X_OFFSET)?;
  let dest_y = read_i32(data, record_offset + EMR_BITMAP_DEST_Y_OFFSET)?;
  let (dest_width, dest_height, raster_operation, source_rect) = match record_type {
    EMR_BIT_BLT => (
      read_i32(data, record_offset + EMR_BLT_DEST_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_BLT_DEST_HEIGHT_OFFSET)?,
      Some(emf_ternary_raster_operation(read_u32(
        data,
        record_offset + EMR_BLT_ROP_OFFSET,
      )?)),
      None,
    ),
    EMR_STRETCH_BLT => (
      read_i32(data, record_offset + EMR_BLT_DEST_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_BLT_DEST_HEIGHT_OFFSET)?,
      Some(emf_ternary_raster_operation(read_u32(
        data,
        record_offset + EMR_BLT_ROP_OFFSET,
      )?)),
      Some((
        read_i32(data, record_offset + EMR_BLT_SOURCE_X_OFFSET)?,
        read_i32(data, record_offset + EMR_BLT_SOURCE_Y_OFFSET)?,
        read_i32(data, record_offset + EMR_STRETCH_BLT_SOURCE_WIDTH_OFFSET)?,
        read_i32(data, record_offset + EMR_STRETCH_BLT_SOURCE_HEIGHT_OFFSET)?,
      )),
    ),
    EMR_SET_DIBITS_TO_DEVICE => (
      read_i32(data, record_offset + EMR_BITMAP_SOURCE_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_BITMAP_SOURCE_HEIGHT_OFFSET)?,
      None,
      None,
    ),
    EMR_STRETCH_DIBITS => (
      read_i32(data, record_offset + EMR_STRETCH_DIBITS_DEST_WIDTH_OFFSET)?,
      read_i32(data, record_offset + EMR_STRETCH_DIBITS_DEST_HEIGHT_OFFSET)?,
      Some(emf_ternary_raster_operation(read_u32(
        data,
        record_offset + EMR_STRETCH_DIBITS_ROP_OFFSET,
      )?)),
      Some((
        read_i32(data, record_offset + EMR_BITMAP_DEST_X_OFFSET + 8)?,
        read_i32(data, record_offset + EMR_BITMAP_DEST_Y_OFFSET + 8)?,
        read_i32(data, record_offset + EMR_BITMAP_SOURCE_WIDTH_OFFSET)?,
        read_i32(data, record_offset + EMR_BITMAP_SOURCE_HEIGHT_OFFSET)?,
      )),
    ),
    _ => unreachable!(),
  };
  if dest_width == 0 || dest_height == 0 {
    return Ok(None);
  }

  Ok(Some(EmfBitmapDrawTarget {
    dest_x,
    dest_y,
    dest_width,
    dest_height,
    raster_operation,
    source_rect,
  }))
}

fn emf_ternary_raster_operation(raw: u32) -> WmfTernaryRasterOperationCode {
  WmfTernaryRasterOperationCode::from_raw(((raw >> 16) & 0xff) as u8)
}

fn decode_bitmap_record_as_raster(
  data: &[u8],
  record_type: u32,
  record_offset: usize,
  record_size: usize,
) -> Result<DecodedMetafile, String> {
  let bitmap = emf_bitmap_record(data, record_type, record_offset, record_size)?
    .ok_or_else(|| "EMF bitmap record omits its source bitmap".to_string())?;
  let dib =
    DeviceIndependentBitmap::from_parts(bitmap.info, bitmap.bits).map_err(|err| err.to_string())?;
  if let Some(format) = dib.embedded_format() {
    return Ok(DecodedMetafile {
      data: dib.bits,
      content_type: format.content_type(),
    });
  }
  let pixels = device_independent_bitmap_to_rgb(&dib, bitmap.color_usage, None)?
    .ok_or_else(|| "unsupported EMF source bitmap format".to_string())?;
  Ok(DecodedMetafile {
    data: rgb_to_png(&pixels.rgb, pixels.width as u32, pixels.height as u32)?,
    content_type: "image/png",
  })
}

#[derive(Clone, Copy)]
struct EmfBitmapRecord<'a> {
  info: &'a [u8],
  bits: &'a [u8],
  color_usage: DibColorUsage,
}

fn emf_bitmap_record<'a>(
  data: &'a [u8],
  record_type: u32,
  record_offset: usize,
  record_size: usize,
) -> Result<Option<EmfBitmapRecord<'a>>, String> {
  let (info_offset_field, info_size_field, bits_offset_field, bits_size_field, usage_field) =
    match record_type {
      EMR_BIT_BLT | EMR_STRETCH_BLT => (
        EMR_BLT_INFO_OFFSET_OFFSET,
        EMR_BLT_INFO_SIZE_OFFSET,
        EMR_BLT_BITS_OFFSET_OFFSET,
        EMR_BLT_BITS_SIZE_OFFSET,
        EMR_BLT_COLOR_USAGE_OFFSET,
      ),
      EMR_SET_DIBITS_TO_DEVICE | EMR_STRETCH_DIBITS => (
        EMR_BITMAP_INFO_OFFSET_OFFSET,
        EMR_BITMAP_INFO_SIZE_OFFSET,
        EMR_BITMAP_BITS_OFFSET_OFFSET,
        EMR_BITMAP_BITS_SIZE_OFFSET,
        EMR_BITMAP_COLOR_USAGE_OFFSET,
      ),
      _ => {
        return Err(format!(
          "unsupported EMF bitmap record type 0x{record_type:08x}"
        ));
      }
    };
  let record_end = record_offset
    .checked_add(record_size)
    .ok_or_else(|| "EMF bitmap record range overflows".to_string())?;
  if record_end > data.len() {
    return Err("EMF bitmap record points outside the file".into());
  }
  let off_bmi = read_u32(data, record_offset + info_offset_field)? as usize;
  let cb_bmi = read_u32(data, record_offset + info_size_field)? as usize;
  let off_bits = read_u32(data, record_offset + bits_offset_field)? as usize;
  let cb_bits = read_u32(data, record_offset + bits_size_field)? as usize;
  if cb_bmi == 0 && cb_bits == 0 {
    return Ok(None);
  }
  let record_slice = |offset: usize, size: usize, description: &str| {
    let end = offset
      .checked_add(size)
      .ok_or_else(|| format!("{description} range overflows"))?;
    if end > record_size {
      return Err(format!("{description} points outside its EMF record"));
    }
    Ok(&data[record_offset + offset..record_offset + end])
  };
  let color_usage_raw = read_u32(data, record_offset + usage_field)?;
  let color_usage = DibColorUsage::from_raw(color_usage_raw)
    .ok_or_else(|| format!("unsupported EMF DIB color usage: {color_usage_raw}"))?;
  Ok(Some(EmfBitmapRecord {
    info: record_slice(off_bmi, cb_bmi, "bitmap info")?,
    bits: record_slice(off_bits, cb_bits, "bitmap bits")?,
    color_usage,
  }))
}

fn decode_bitmap_record_as_rgb(
  data: &[u8],
  record_type: u32,
  record_offset: usize,
  record_size: usize,
) -> Result<Option<RasterPixels>, String> {
  let Some(bitmap) = emf_bitmap_record(data, record_type, record_offset, record_size)? else {
    return Ok(None);
  };
  let dib =
    DeviceIndependentBitmap::from_parts(bitmap.info, bitmap.bits).map_err(|err| err.to_string())?;
  device_independent_bitmap_to_rgb(&dib, bitmap.color_usage, None)
}

fn crop_raster_pixels(
  image: &RasterPixels,
  (x, y, width, height): (i32, i32, i32, i32),
) -> Option<RasterPixels> {
  let (x, y, width, height) = (
    usize::try_from(x).ok()?,
    usize::try_from(y).ok()?,
    usize::try_from(width).ok()?,
    usize::try_from(height).ok()?,
  );
  if width == 0 || height == 0 {
    return None;
  }
  let right = x.checked_add(width)?;
  let bottom = y.checked_add(height)?;
  if right > image.width || bottom > image.height {
    return None;
  }
  if x == 0 && y == 0 && width == image.width && height == image.height {
    return None;
  }
  let mut rgb = Vec::with_capacity(width * height * RGB_BYTES_PER_PIXEL);
  for row in y..bottom {
    let start = (row * image.width + x) * RGB_BYTES_PER_PIXEL;
    let end = start + width * RGB_BYTES_PER_PIXEL;
    rgb.extend_from_slice(&image.rgb[start..end]);
  }
  Some(RasterPixels { width, height, rgb })
}

fn process_emf_plus_comment(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
  state: &mut EmfVectorState,
) -> Result<Option<EmfPlusCommentControl>, String> {
  // EMR_COMMENT_EMFPLUS chunks as a stream of 12-byte EMF+ record headers.
  let data_size = read_u32(data, record_offset + 8)? as usize;
  let comment_identifier = read_u32(data, record_offset + 12)?;
  if comment_identifier != EMR_COMMENT_EMFPLUS || data_size < 4 {
    return Ok(None);
  }
  let mut control = EmfPlusCommentControl::default();
  let mut cursor = record_offset + 16;
  let end = record_offset
    .checked_add(12)
    .and_then(|offset| offset.checked_add(data_size))
    .map(|end| end.min(record_offset + record_size))
    .ok_or_else(|| "EMF+ comment range overflows".to_string())?;
  while cursor + 12 <= end {
    let size = read_u32(data, cursor + 4)? as usize;
    if size < 12 || cursor + size > end {
      break;
    }
    let record_bytes = &data[cursor..cursor + size];
    let mut reader = Reader::new(std::io::Cursor::new(record_bytes));
    if let Ok(record) = EmfPlusRecord::read_from(&mut reader, record_bytes.len() as u64) {
      let record_kind = record.record_kind();
      let record_flags = record.flags();
      control.header |= record_kind == Some(EmfPlusRecordType::Header);
      control.get_dc |= record_kind == Some(EmfPlusRecordType::GetDc);
      if record_kind == Some(EmfPlusRecordType::Object) {
        if let Ok(fragment) = record.into_object_fragment() {
          process_emf_plus_object(fragment, state)?;
        }
      } else if let Ok(parsed) = record.parse_data_relaxed() {
        process_emf_plus_record(parsed, record_flags, state)?;
      }
    }
    cursor += size;
  }
  Ok(Some(control))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EmfPlusCommentControl {
  header: bool,
  get_dc: bool,
}

fn process_emf_plus_record(
  record: EmfPlusRecordData<'_>,
  flags: EmfPlusRecordFlags,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  match record {
    EmfPlusRecordData::Object(value) => process_emf_plus_object(value, state)?,
    EmfPlusRecordData::Header(value) => {
      state.emf_plus_logical_dpi_x = value.logical_dpi_x.max(1) as f32;
      state.emf_plus_logical_dpi_y = value.logical_dpi_y.max(1) as f32;
      state.emf_plus_video_display = value.video_display();
      state.emf_plus_page_unit = EmfPlusUnitType::Display;
      state.emf_plus_page_scale = 1.0;
    }
    EmfPlusRecordData::Clear(value) => {
      let color = emf_plus_argb_to_render_color(value.color);
      for y in 0..state.height {
        for x in 0..state.width {
          state.set_pixel_with_alpha(x as i32, y as i32, color.color, color.alpha);
        }
      }
    }
    EmfPlusRecordData::FillRects(value) => {
      let Some(brush) = emf_plus_brush_ref(value.brush, state) else {
        return Ok(());
      };
      for rect in value.rects {
        let (left, top, right, bottom) = emf_plus_rect_bounds(rect);
        state.fill_polygon_with_emf_plus_brush(&rect_points(left, top, right, bottom), &brush);
      }
    }
    EmfPlusRecordData::DrawRects(value) => {
      let old = state.current_pen;
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        state.current_pen = Some(pen);
        for rect in value.rects {
          let (left, top, right, bottom) = emf_plus_rect_bounds(rect);
          state.fill_rect(left, top, right, bottom);
        }
      }
      state.current_pen = old;
    }
    EmfPlusRecordData::FillPolygon(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
        let points = emf_plus_points_to_emf_points(&value.points);
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
      }
    }
    EmfPlusRecordData::DrawLines(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        state.draw_polyline(&points, value.close_shape);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::FillEllipse(value) => fill_emf_plus_rect_shape(value, state),
    EmfPlusRecordData::DrawEllipse(value) => draw_emf_plus_rect_shape(value, state),
    EmfPlusRecordData::FillPie(value) => fill_emf_plus_pie(value, state),
    EmfPlusRecordData::DrawPie(value) | EmfPlusRecordData::DrawArc(value) => {
      draw_emf_plus_arc(value, state)
    }
    EmfPlusRecordData::FillRegion(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state)
        && let Some(region) = emf_plus_region(value.object_id, state)
      {
        state.fill_emf_plus_region(&region, &brush);
      }
    }
    EmfPlusRecordData::FillPath(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state)
        && let Some(points) = emf_plus_path_points(value.object_id, state)
      {
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
      }
    }
    EmfPlusRecordData::DrawPath(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state)
        && let Some(points) = emf_plus_path_points(value.object_id, state)
      {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        state.draw_polyline(&points, true);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::FillClosedCurve(value) => {
      if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
        let points = emf_plus_points_to_emf_points(&value.points);
        let points = flatten_cardinal_curve(&points, value.tension, true);
        state.fill_polygon_with_emf_plus_brush(&points, &brush);
        state.draw_polyline(&points, true);
      }
    }
    EmfPlusRecordData::DrawClosedCurve(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        let points = flatten_cardinal_curve(&points, value.tension, true);
        state.draw_polyline(&points, true);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::DrawCurve(value) => {
      if let Some(pen) = emf_plus_pen(value.pen_id, state) {
        let old = state.current_pen;
        state.current_pen = Some(pen);
        let points = emf_plus_points_to_emf_points(&value.points);
        let start = value.offset as usize;
        let end = start
          .saturating_add(value.num_segments as usize + 1)
          .min(points.len());
        let points = if start < end {
          flatten_cardinal_curve(&points[start..end], value.tension, false)
        } else {
          points
        };
        state.draw_polyline(&points, false);
        state.current_pen = old;
      }
    }
    EmfPlusRecordData::StrokeFillPath => {
      let paths = state
        .emf_plus_objects
        .iter()
        .filter_map(|object| match object {
          Some(EmfPlusRenderObject::Path(points)) => Some(points.clone()),
          _ => None,
        })
        .collect::<Vec<_>>();
      for points in paths {
        state.fill_polygon(&points);
        state.draw_polyline(&points, true);
      }
    }
    EmfPlusRecordData::DrawBeziers(value) => draw_emf_plus_beziers(value, state),
    EmfPlusRecordData::DrawImage(value) => draw_emf_plus_image(value, state),
    EmfPlusRecordData::DrawImagePoints(value) => draw_emf_plus_image_points(value, state),
    EmfPlusRecordData::DrawString(value) => draw_emf_plus_string(value, state),
    EmfPlusRecordData::DrawDriverString(value) => {
      if let Some(color) = emf_plus_brush_ref_to_color(value.brush, state)
        && let Some(first) = value.glyph_positions.first()
      {
        let text = value
          .glyphs
          .iter()
          .filter_map(|glyph| char::from_u32(u32::from(*glyph)))
          .collect::<String>();
        state.draw_text(first.x as i32, first.y as i32, &text, color.color, 12);
      }
    }
    EmfPlusRecordData::Save(value) => state.save_emf_plus_state(value.stack_index, false),
    EmfPlusRecordData::Restore(value) => state.restore_emf_plus_state(value.stack_index, false),
    EmfPlusRecordData::BeginContainer(value) => {
      state.save_emf_plus_state(value.stack_index, true);
    }
    EmfPlusRecordData::BeginContainerNoParams(value) => {
      state.save_emf_plus_state(value.stack_index, true);
    }
    EmfPlusRecordData::EndContainer(value) => {
      state.restore_emf_plus_state(value.stack_index, true);
    }
    EmfPlusRecordData::SetClipRect(value) => {
      let (left, top, right, bottom) = emf_plus_rectf_bounds(value.clip_rect);
      let points = [
        EmfPoint { x: left, y: top },
        EmfPoint { x: right, y: top },
        EmfPoint {
          x: right,
          y: bottom,
        },
        EmfPoint { x: left, y: bottom },
      ];
      state.set_clip_points_logical(&points, value.combine_mode);
    }
    EmfPlusRecordData::SetClipPath(value) => {
      if let Some(points) = emf_plus_path_points(value.object_id, state) {
        state.set_clip_points_logical(&points, value.combine_mode);
      }
    }
    EmfPlusRecordData::SetClipRegion(value) => {
      if let Some(region) = emf_plus_region(value.object_id, state) {
        state.set_clip_region(&region, value.combine_mode);
      }
    }
    EmfPlusRecordData::OffsetClip(value) => {
      state.offset_clip(value.dx, value.dy);
    }
    EmfPlusRecordData::ResetClip => {
      state.clip_rect = None;
      state.clip_mask = None;
    }
    EmfPlusRecordData::SetWorldTransform(value) => {
      state.world_transform = xform_to_transform(value);
    }
    EmfPlusRecordData::ResetWorldTransform => state.world_transform = EmfTransform::identity(),
    EmfPlusRecordData::MultiplyWorldTransform(value) => {
      multiply_emf_plus_transform(xform_to_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::TranslateWorldTransform(value) => {
      multiply_emf_plus_transform(translate_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::ScaleWorldTransform(value) => {
      multiply_emf_plus_transform(scale_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::RotateWorldTransform(value) => {
      multiply_emf_plus_transform(rotate_transform(value.data), value.post_multiply, state);
    }
    EmfPlusRecordData::SetPageTransform(value)
      if value.page_scale.is_finite() && value.page_scale > 0.0 =>
    {
      if let Some(page_unit) = flags.page_unit() {
        // [MS-EMFPLUS] 2.3.9.5 defines a page-space-to-device-space
        // property, not a world-transform mutation. GDI+ emits a complete
        // replacement record whenever either PageUnit or PageScale changes.
        state.emf_plus_page_unit = page_unit;
        state.emf_plus_page_scale = value.page_scale;
      }
    }
    EmfPlusRecordData::SetTsGraphics(value) => {
      state.world_transform = xform_to_transform(value.world_to_device);
    }
    _ => {}
  }
  Ok(())
}

fn process_emf_plus_object(
  value: EmfPlusObjectRecordData,
  state: &mut EmfVectorState,
) -> Result<(), String> {
  match state.emf_plus_object_assembler.push_relaxed(value) {
    Ok(Some(complete)) => process_complete_emf_plus_object(complete, state),
    Ok(None) => {}
    Err(_) => {
      state.emf_plus_object_assembler = EmfPlusObjectAssembler::default();
    }
  }
  Ok(())
}

fn process_complete_emf_plus_object(value: EmfPlusObjectRecordData, state: &mut EmfVectorState) {
  let object = match value.parse_object_data_relaxed() {
    Ok(EmfPlusObjectData::Brush(brush)) => {
      EmfPlusRenderObject::Brush(emf_plus_brush_object(&brush))
    }
    Ok(EmfPlusObjectData::Pen(pen)) => EmfPlusRenderObject::Pen(emf_plus_pen_object(&pen)),
    Ok(EmfPlusObjectData::Path(path)) => {
      EmfPlusRenderObject::Path(emf_plus_path_object_points(&path))
    }
    Ok(EmfPlusObjectData::Region(region)) => emf_plus_region_object(&region)
      .map(EmfPlusRenderObject::Region)
      .unwrap_or(EmfPlusRenderObject::Unsupported),
    Ok(EmfPlusObjectData::Image(image)) => match emf_plus_image_object_to_rgb(&image) {
      Ok(Some(image)) => EmfPlusRenderObject::Image(image),
      _ => EmfPlusRenderObject::Unsupported,
    },
    Ok(EmfPlusObjectData::Font(font)) => EmfPlusRenderObject::Font(font),
    _ => EmfPlusRenderObject::Unsupported,
  };
  let index = value.object_id as usize;
  if state.emf_plus_objects.len() <= index {
    state.emf_plus_objects.resize(index + 1, None);
  }
  state.emf_plus_objects[index] = Some(object);
}

fn emf_plus_brush_object(brush: &crate::emfplus::EmfPlusBrushObject) -> Option<EmfPlusRenderBrush> {
  match brush.parse_brush_data_relaxed().ok()? {
    EmfPlusBrushData::Solid(value) => Some(EmfPlusRenderBrush::Solid(
      emf_plus_argb_to_render_color(value.solid_color),
    )),
    EmfPlusBrushData::Hatch(value) => Some(EmfPlusRenderBrush::Hatch {
      fore: emf_plus_argb_to_render_color(value.fore_color),
      back: emf_plus_argb_to_render_color(value.back_color),
      style: value.hatch_style,
    }),
    EmfPlusBrushData::LinearGradient(value) => Some(EmfPlusRenderBrush::LinearGradient {
      rect: (
        value.rect.x,
        value.rect.y,
        value.rect.x + value.rect.width,
        value.rect.y + value.rect.height,
      ),
      start: emf_plus_argb_to_render_color(value.start_color),
      end: emf_plus_argb_to_render_color(value.end_color),
    }),
    EmfPlusBrushData::PathGradient(value) => Some(EmfPlusRenderBrush::PathGradient {
      center: (value.center_point.x, value.center_point.y),
      center_color: emf_plus_argb_to_render_color(value.center_color),
      surround: value
        .surrounding_colors
        .first()
        .copied()
        .map(emf_plus_argb_to_render_color)
        .unwrap_or_else(|| emf_plus_argb_to_render_color(value.center_color)),
    }),
    EmfPlusBrushData::Texture(value) => value
      .parse_optional_data()
      .ok()?
      .image_object
      .as_ref()
      .and_then(|image| emf_plus_image_object_to_rgb(image).ok().flatten())
      .map(EmfPlusRenderBrush::Texture),
    EmfPlusBrushData::Unknown { .. } => None,
  }
}

fn emf_plus_pen_object(pen: &EmfPlusPenObject) -> Option<EmfPen> {
  let payload = pen.parse_pen_payload_relaxed().ok()?;
  let brush = payload.brush_object.as_ref()?;
  let color = emf_plus_brush_object(brush)?.representative_color();
  Some(EmfPen {
    color: color.color,
    alpha: color.alpha,
    width: payload.pen_data.pen_width.round().max(1.0) as usize,
    width_space: if payload.pen_data.pen_unit_kind() == Some(EmfPlusUnitType::World) {
      EmfPenWidthSpace::World
    } else {
      EmfPenWidthSpace::Device
    },
  })
}

fn emf_plus_brush_ref(
  brush: EmfPlusBrushRef,
  state: &EmfVectorState,
) -> Option<EmfPlusRenderBrush> {
  match brush {
    EmfPlusBrushRef::Color(color) => Some(EmfPlusRenderBrush::Solid(
      emf_plus_argb_to_render_color(color),
    )),
    EmfPlusBrushRef::ObjectId(id) => match state.emf_plus_objects.get(id as usize)? {
      Some(EmfPlusRenderObject::Brush(brush)) => brush.clone(),
      Some(EmfPlusRenderObject::Pen(Some(pen))) => {
        Some(EmfPlusRenderBrush::Solid(EmfPlusRenderColor {
          color: pen.color,
          alpha: pen.alpha,
        }))
      }
      _ => None,
    },
  }
}

fn emf_plus_brush_ref_to_color(
  brush: EmfPlusBrushRef,
  state: &EmfVectorState,
) -> Option<EmfPlusRenderColor> {
  emf_plus_brush_ref(brush, state).map(|brush| brush.representative_color())
}

fn emf_plus_pen(id: u8, state: &EmfVectorState) -> Option<EmfPen> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Pen(pen)) => pen.map(|pen| state.resolve_pen(pen)),
    Some(EmfPlusRenderObject::Brush(Some(brush))) => {
      let color = brush.representative_color();
      Some(EmfPen {
        color: color.color,
        alpha: color.alpha,
        width: 1,
        width_space: EmfPenWidthSpace::Device,
      })
    }
    _ => None,
  }
}

fn emf_plus_argb_to_render_color(color: crate::EmfPlusArgb) -> EmfPlusRenderColor {
  EmfPlusRenderColor {
    color: EmfColor {
      r: color.red,
      g: color.green,
      b: color.blue,
    },
    alpha: color.alpha,
  }
}

fn lerp_emf_plus_color(
  start: EmfPlusRenderColor,
  end: EmfPlusRenderColor,
  t: f32,
) -> EmfPlusRenderColor {
  let t = t.clamp(0.0, 1.0);
  EmfPlusRenderColor {
    color: lerp_color(start.color, end.color, t),
    alpha: (start.alpha as f32 + (end.alpha as f32 - start.alpha as f32) * t).round() as u8,
  }
}

fn average_emf_plus_color(a: EmfPlusRenderColor, b: EmfPlusRenderColor) -> EmfPlusRenderColor {
  EmfPlusRenderColor {
    color: average_color(a.color, b.color),
    alpha: ((u16::from(a.alpha) + u16::from(b.alpha)) / 2) as u8,
  }
}

fn lerp_color(start: EmfColor, end: EmfColor, t: f32) -> EmfColor {
  let t = t.clamp(0.0, 1.0);
  EmfColor {
    r: (start.r as f32 + (end.r as f32 - start.r as f32) * t).round() as u8,
    g: (start.g as f32 + (end.g as f32 - start.g as f32) * t).round() as u8,
    b: (start.b as f32 + (end.b as f32 - start.b as f32) * t).round() as u8,
  }
}

fn average_color(a: EmfColor, b: EmfColor) -> EmfColor {
  EmfColor {
    r: ((u16::from(a.r) + u16::from(b.r)) / 2) as u8,
    g: ((u16::from(a.g) + u16::from(b.g)) / 2) as u8,
    b: ((u16::from(a.b) + u16::from(b.b)) / 2) as u8,
  }
}

fn average_image_color(image: &RasterPixels) -> EmfColor {
  if image.rgb.is_empty() {
    return EmfColor { r: 0, g: 0, b: 0 };
  }
  let mut r = 0u64;
  let mut g = 0u64;
  let mut b = 0u64;
  let mut count = 0u64;
  for pixel in image.rgb.chunks_exact(RGB_BYTES_PER_PIXEL) {
    r += u64::from(pixel[0]);
    g += u64::from(pixel[1]);
    b += u64::from(pixel[2]);
    count += 1;
  }
  EmfColor {
    r: (r / count) as u8,
    g: (g / count) as u8,
    b: (b / count) as u8,
  }
}

fn emf_plus_rect_bounds(rect: crate::EmfPlusRect) -> (i32, i32, i32, i32) {
  match rect {
    crate::EmfPlusRect::Compressed(rect) => (
      i32::from(rect.x),
      i32::from(rect.y),
      i32::from(rect.x) + i32::from(rect.width),
      i32::from(rect.y) + i32::from(rect.height),
    ),
    crate::EmfPlusRect::Float(rect) => emf_plus_rectf_bounds(rect),
  }
}

fn emf_plus_rectf_bounds(rect: crate::RectF) -> (i32, i32, i32, i32) {
  (
    rect.x.round() as i32,
    rect.y.round() as i32,
    (rect.x + rect.width).round() as i32,
    (rect.y + rect.height).round() as i32,
  )
}

fn fill_emf_plus_rect_shape(value: EmfPlusFillRectShapeData, state: &mut EmfVectorState) {
  if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_ellipse_with_emf_plus_brush(left, top, right, bottom, &brush);
  }
}

fn draw_emf_plus_rect_shape(value: EmfPlusDrawRectShapeData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old_brush = state.current_brush;
    let old_pen = state.current_pen;
    state.current_brush = None;
    state.current_pen = Some(pen);
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_ellipse(left, top, right, bottom);
    state.current_brush = old_brush;
    state.current_pen = old_pen;
  }
}

fn fill_emf_plus_pie(value: EmfPlusFillPieData, state: &mut EmfVectorState) {
  if let Some(brush) = emf_plus_brush_ref(value.brush, state) {
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    let points = arc_segment_points(
      left,
      top,
      right,
      bottom,
      value.start_angle,
      value.sweep_angle,
      true,
    );
    state.fill_polygon_with_emf_plus_brush(&points, &brush);
    state.draw_polyline(&points, true);
  }
}

fn draw_emf_plus_arc(value: EmfPlusDrawArcData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old = state.current_pen;
    state.current_pen = Some(pen);
    let (left, top, right, bottom) = emf_plus_rect_bounds(value.rect);
    state.fill_arc_segment(
      (left, top, right, bottom),
      value.start_angle,
      value.sweep_angle,
      false,
    );
    state.current_pen = old;
  }
}

fn arc_segment_points(
  left: i32,
  top: i32,
  right: i32,
  bottom: i32,
  start_angle: f32,
  sweep_angle: f32,
  pie: bool,
) -> Vec<EmfPoint> {
  if !start_angle.is_finite() || !sweep_angle.is_finite() {
    return Vec::new();
  }
  // [MS-EMFPLUS] requires StartAngle to be interpreted modulo 360 and
  // SweepAngle to be clamped to one revolution. LibreOffice emits negative
  // StartAngle values in otherwise valid EMF+ streams, so playback applies
  // the required interpretation even when strict validation rejects the
  // producer's non-conforming sign.
  let start_angle = start_angle.rem_euclid(360.0);
  let sweep_angle = sweep_angle.clamp(-360.0, 360.0);
  let steps = ((sweep_angle.abs() / 5.0).ceil() as usize).clamp(6, 144);
  let cx = (left + right) as f32 / 2.0;
  let cy = (top + bottom) as f32 / 2.0;
  let rx = (right - left).abs() as f32 / 2.0;
  let ry = (bottom - top).abs() as f32 / 2.0;
  let mut points = Vec::with_capacity(steps + usize::from(pie) + 1);
  if pie {
    points.push(EmfPoint {
      x: cx.round() as i32,
      y: cy.round() as i32,
    });
  }
  for index in 0..=steps {
    let angle = (start_angle + sweep_angle * index as f32 / steps as f32).to_radians();
    points.push(EmfPoint {
      x: (cx + angle.cos() * rx).round() as i32,
      y: (cy + angle.sin() * ry).round() as i32,
    });
  }
  points
}

fn draw_emf_plus_beziers(value: EmfPlusDrawPointsData, state: &mut EmfVectorState) {
  if let Some(pen) = emf_plus_pen(value.pen_id, state) {
    let old = state.current_pen;
    state.current_pen = Some(pen);
    let points = emf_plus_points_to_emf_points(&value.points);
    let flattened = flatten_bezier_sequence(&points);
    state.draw_polyline(&flattened, false);
    state.current_pen = old;
  }
}

fn emf_plus_points_to_emf_points(points: &EmfPlusPointData) -> Vec<EmfPoint> {
  match points {
    EmfPlusPointData::Relative(points) => {
      let mut current = EmfPoint { x: 0, y: 0 };
      points
        .iter()
        .map(|point| {
          current.x += i32::from(point.x);
          current.y += i32::from(point.y);
          current
        })
        .collect()
    }
    EmfPlusPointData::Compressed(points) => points
      .iter()
      .map(|point| EmfPoint {
        x: i32::from(point.x),
        y: i32::from(point.y),
      })
      .collect(),
    EmfPlusPointData::Float(points) => points
      .iter()
      .map(|point| EmfPoint {
        x: point.x.round() as i32,
        y: point.y.round() as i32,
      })
      .collect(),
  }
}

fn emf_plus_path_object_points(path: &EmfPlusPathObject) -> Vec<EmfPoint> {
  let points = emf_plus_points_to_emf_points(&path.points);
  let types = expanded_path_point_types(&path.point_types);
  if types.is_empty() {
    return points;
  }
  let mut result = Vec::with_capacity(points.len());
  let mut index = 0usize;
  while index < points.len() && index < types.len() {
    let point = points[index];
    let point_type = types[index];
    if point_type.path_point_type() == Some(EmfPlusPathPointType::Bezier)
      && index + 2 < points.len()
      && let Some(start) = result.last().copied()
    {
      result.extend(sample_cubic_bezier(
        start,
        points[index],
        points[index + 1],
        points[index + 2],
      ));
      index += 3;
      continue;
    }
    result.push(point);
    if point_type
      .path_point_flags()
      .contains(EmfPlusPathPointTypeFlags::CLOSE_SUBPATH)
      && let Some(first) = result.first().copied()
    {
      result.push(first);
    }
    index += 1;
  }
  result
}

fn flatten_bezier_sequence(points: &[EmfPoint]) -> Vec<EmfPoint> {
  let Some(first) = points.first().copied() else {
    return Vec::new();
  };
  let mut result = vec![first];
  let mut index = 1usize;
  while index + 2 < points.len() {
    let start = *result.last().unwrap_or(&first);
    result.extend(sample_cubic_bezier(
      start,
      points[index],
      points[index + 1],
      points[index + 2],
    ));
    index += 3;
  }
  result.extend_from_slice(&points[index..]);
  result
}

fn sample_cubic_bezier(p0: EmfPoint, p1: EmfPoint, p2: EmfPoint, p3: EmfPoint) -> Vec<EmfPoint> {
  let chord = ((p3.x - p0.x).unsigned_abs() + (p3.y - p0.y).unsigned_abs()) as usize;
  let control = ((p1.x - p0.x).unsigned_abs()
    + (p1.y - p0.y).unsigned_abs()
    + (p2.x - p3.x).unsigned_abs()
    + (p2.y - p3.y).unsigned_abs()) as usize;
  let steps = ((chord + control) / 16).clamp(8, 64);
  (1..=steps)
    .map(|step| {
      let t = step as f32 / steps as f32;
      let mt = 1.0 - t;
      let x = mt.powi(3) * p0.x as f32
        + 3.0 * mt.powi(2) * t * p1.x as f32
        + 3.0 * mt * t.powi(2) * p2.x as f32
        + t.powi(3) * p3.x as f32;
      let y = mt.powi(3) * p0.y as f32
        + 3.0 * mt.powi(2) * t * p1.y as f32
        + 3.0 * mt * t.powi(2) * p2.y as f32
        + t.powi(3) * p3.y as f32;
      EmfPoint {
        x: x.round() as i32,
        y: y.round() as i32,
      }
    })
    .collect()
}

fn flatten_cardinal_curve(points: &[EmfPoint], tension: f32, closed: bool) -> Vec<EmfPoint> {
  if points.len() < 2 {
    return points.to_vec();
  }
  let mut result = Vec::new();
  if !closed {
    result.push(points[0]);
  }
  let segment_count = if closed {
    points.len()
  } else {
    points.len() - 1
  };
  let tension = tension.clamp(0.0, 1.0);
  let tangent_scale = (1.0 - tension) / 2.0;
  for index in 0..segment_count {
    let p0 = if index == 0 {
      if closed {
        points[points.len() - 1]
      } else {
        points[0]
      }
    } else {
      points[index - 1]
    };
    let p1 = points[index];
    let p2 = points[(index + 1) % points.len()];
    let p3 = if index + 2 < points.len() {
      points[index + 2]
    } else if closed {
      points[(index + 2) % points.len()]
    } else {
      points[points.len() - 1]
    };
    let distance = ((p2.x - p1.x).unsigned_abs() + (p2.y - p1.y).unsigned_abs()) as usize;
    let steps = (distance / 12).clamp(6, 32);
    for step in 1..=steps {
      let t = step as f32 / steps as f32;
      let t2 = t * t;
      let t3 = t2 * t;
      let m1x = (p2.x - p0.x) as f32 * tangent_scale;
      let m1y = (p2.y - p0.y) as f32 * tangent_scale;
      let m2x = (p3.x - p1.x) as f32 * tangent_scale;
      let m2y = (p3.y - p1.y) as f32 * tangent_scale;
      let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
      let h10 = t3 - 2.0 * t2 + t;
      let h01 = -2.0 * t3 + 3.0 * t2;
      let h11 = t3 - t2;
      result.push(EmfPoint {
        x: (h00 * p1.x as f32 + h10 * m1x + h01 * p2.x as f32 + h11 * m2x).round() as i32,
        y: (h00 * p1.y as f32 + h10 * m1y + h01 * p2.y as f32 + h11 * m2y).round() as i32,
      });
    }
  }
  result
}

fn expanded_path_point_types(types: &EmfPlusPathPointTypes) -> Vec<EmfPlusPathPointTypeValue> {
  match types {
    EmfPlusPathPointTypes::Values(values) => values.clone(),
    EmfPlusPathPointTypes::Rle(values) => {
      let mut expanded = Vec::new();
      for value in values {
        expanded.extend(std::iter::repeat_n(
          value.point_type,
          value.run_count() as usize,
        ));
      }
      expanded
    }
  }
}

fn emf_plus_path_points(id: u8, state: &EmfVectorState) -> Option<Vec<EmfPoint>> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Path(points)) => Some(points.clone()),
    _ => None,
  }
}

fn emf_plus_region(id: u8, state: &EmfVectorState) -> Option<EmfPlusRenderRegion> {
  match state.emf_plus_objects.get(id as usize)? {
    Some(EmfPlusRenderObject::Region(region)) => Some(region.clone()),
    _ => None,
  }
}

fn emf_plus_region_object(
  region: &crate::emfplus::EmfPlusRegionObject,
) -> Option<EmfPlusRenderRegion> {
  region
    .parse_region_nodes()
    .ok()?
    .first()
    .and_then(emf_plus_region_node)
}

fn emf_plus_region_node(node: &crate::emfplus::EmfPlusRegionNode) -> Option<EmfPlusRenderRegion> {
  match &node.data {
    crate::emfplus::EmfPlusRegionNodeData::Rect(rect) => {
      let (left, top, right, bottom) = emf_plus_rectf_bounds(*rect);
      Some(EmfPlusRenderRegion::Polygon(rect_points(
        left, top, right, bottom,
      )))
    }
    crate::emfplus::EmfPlusRegionNodeData::Path(path) => path
      .path()
      .map(emf_plus_path_object_points)
      .map(EmfPlusRenderRegion::Polygon),
    crate::emfplus::EmfPlusRegionNodeData::Empty => Some(EmfPlusRenderRegion::Empty),
    crate::emfplus::EmfPlusRegionNodeData::Infinite => Some(EmfPlusRenderRegion::Infinite),
    crate::emfplus::EmfPlusRegionNodeData::ChildNodes(children) => {
      let mode = match node.node_type_kind()? {
        EmfPlusRegionNodeDataType::And => 1,
        EmfPlusRegionNodeDataType::Or => 2,
        EmfPlusRegionNodeDataType::Xor => 3,
        EmfPlusRegionNodeDataType::Exclude => 4,
        EmfPlusRegionNodeDataType::Complement => 5,
        _ => return None,
      };
      Some(EmfPlusRenderRegion::Combine {
        mode,
        left: Box::new(emf_plus_region_node(&children.left)?),
        right: Box::new(emf_plus_region_node(&children.right)?),
      })
    }
    crate::emfplus::EmfPlusRegionNodeData::Raw(_) => None,
  }
}

fn rect_points(left: i32, top: i32, right: i32, bottom: i32) -> Vec<EmfPoint> {
  vec![
    EmfPoint { x: left, y: top },
    EmfPoint { x: right, y: top },
    EmfPoint {
      x: right,
      y: bottom,
    },
    EmfPoint { x: left, y: bottom },
  ]
}

fn draw_emf_plus_image(value: EmfPlusDrawImageData, state: &mut EmfVectorState) {
  let Some(image) = state
    .emf_plus_objects
    .get(value.image_id as usize)
    .and_then(|value| match value {
      Some(EmfPlusRenderObject::Image(image)) => Some(image.clone()),
      _ => None,
    })
  else {
    return;
  };
  let (left, top, right, bottom) = emf_plus_rect_bounds(value.dest_rect);
  state.draw_rgb_image(left, top, right - left, bottom - top, &image);
}

fn draw_emf_plus_image_points(value: EmfPlusDrawImagePointsData, state: &mut EmfVectorState) {
  let Some(image) = state
    .emf_plus_objects
    .get(value.image_id as usize)
    .and_then(|value| match value {
      Some(EmfPlusRenderObject::Image(image)) => Some(image.clone()),
      _ => None,
    })
  else {
    return;
  };
  let points = emf_plus_points_to_emf_points(&value.points);
  let Some(first) = points.first().copied() else {
    return;
  };
  let width = points
    .get(1)
    .map(|point| point.x - first.x)
    .unwrap_or(image.width as i32);
  let height = points
    .get(2)
    .map(|point| point.y - first.y)
    .unwrap_or(image.height as i32);
  state.draw_rgb_image(first.x, first.y, width, height, &image);
}

fn draw_emf_plus_string(value: EmfPlusDrawStringData, state: &mut EmfVectorState) {
  let Some(color) = emf_plus_brush_ref_to_color(value.brush, state) else {
    return;
  };
  let text = value
    .string
    .as_str()
    .map(|text| text.to_string())
    .unwrap_or_default();
  let height = match state.emf_plus_objects.get(value.font_id as usize) {
    Some(Some(EmfPlusRenderObject::Font(font))) => font.em_size.round() as i32,
    _ => value.layout_rect.height.round() as i32,
  }
  .abs()
  .max(7);
  state.draw_text(
    value.layout_rect.x.round() as i32,
    value.layout_rect.y.round() as i32 + height,
    &text,
    color.color,
    height,
  );
}

fn emf_plus_image_object_to_rgb(
  image: &EmfPlusImageObject,
) -> Result<Option<RasterPixels>, String> {
  match image.parse_image_data().map_err(|err| err.to_string())? {
    EmfPlusImageData::Bitmap(bitmap) => emf_plus_bitmap_to_rgb(&bitmap),
    EmfPlusImageData::Metafile(metafile) => {
      let Some(raster) =
        decode_metafile_as_raster(&metafile.metafile_data, None).map_err(|err| err.to_string())?
      else {
        return Ok(None);
      };
      decoded_raster_to_rgb(&raster)
    }
    EmfPlusImageData::Unknown { .. } => Ok(None),
  }
}

fn emf_plus_bitmap_to_rgb(
  bitmap: &crate::emfplus::EmfPlusBitmapObject,
) -> Result<Option<RasterPixels>, String> {
  match bitmap.parse_bitmap_data().map_err(|err| err.to_string())? {
    EmfPlusBitmapPayload::Compressed(data) => {
      let raster = image::load_from_memory(&data.compressed_image_data)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = raster.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: raster.into_raw(),
      }))
    }
    EmfPlusBitmapPayload::Pixel(data) => emf_plus_pixel_bitmap_to_rgb(bitmap, &data.pixel_data),
    EmfPlusBitmapPayload::Unknown { .. } => Ok(None),
  }
}

fn emf_plus_pixel_bitmap_to_rgb(
  bitmap: &crate::emfplus::EmfPlusBitmapObject,
  pixels: &[u8],
) -> Result<Option<RasterPixels>, String> {
  if bitmap.width <= 0 || bitmap.height <= 0 {
    return Ok(None);
  }
  let width = bitmap.width as usize;
  let height = bitmap.height as usize;
  let stride = bitmap.stride.unsigned_abs() as usize;
  let bpp = bitmap.bits_per_pixel();
  let bytes_per_pixel = match bpp {
    24 => 3,
    32 => 4,
    _ => return Ok(None),
  };
  let required = stride
    .checked_mul(height)
    .ok_or_else(|| "EMF+ bitmap dimensions overflow".to_string())?;
  if pixels.len() < required || stride < width * bytes_per_pixel {
    return Err("EMF+ bitmap payload is truncated".to_string());
  }
  let mut rgb = vec![0u8; width * height * RGB_BYTES_PER_PIXEL];
  for row in 0..height {
    let src_row = if bitmap.stride < 0 {
      height - 1 - row
    } else {
      row
    };
    let src = &pixels[src_row * stride..src_row * stride + stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      let src_pixel = &src[col * bytes_per_pixel..col * bytes_per_pixel + bytes_per_pixel];
      let dest_pixel =
        &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
      dest_pixel[0] = src_pixel[2];
      dest_pixel[1] = src_pixel[1];
      dest_pixel[2] = src_pixel[0];
    }
  }
  Ok(Some(RasterPixels { width, height, rgb }))
}

fn xform_to_transform(value: crate::XForm) -> EmfTransform {
  EmfTransform {
    m11: value.m11,
    m12: value.m12,
    m21: value.m21,
    m22: value.m22,
    dx: value.dx,
    dy: value.dy,
  }
}

fn translate_transform(value: EmfPlusTranslateWorldTransformData) -> EmfTransform {
  EmfTransform {
    dx: value.dx,
    dy: value.dy,
    ..EmfTransform::identity()
  }
}

fn scale_transform(value: EmfPlusScaleWorldTransformData) -> EmfTransform {
  EmfTransform {
    m11: value.sx,
    m22: value.sy,
    ..EmfTransform::identity()
  }
}

fn rotate_transform(value: EmfPlusRotateWorldTransformData) -> EmfTransform {
  let radians = value.angle.to_radians();
  EmfTransform {
    m11: radians.cos(),
    m12: radians.sin(),
    m21: -radians.sin(),
    m22: radians.cos(),
    dx: 0.0,
    dy: 0.0,
  }
}

fn multiply_emf_plus_transform(
  transform: EmfTransform,
  post_multiply: bool,
  state: &mut EmfVectorState,
) {
  state.world_transform = if post_multiply {
    state.world_transform.multiply(transform)
  } else {
    transform.multiply(state.world_transform)
  };
}

fn color_ref_to_emf(value: crate::ColorRef) -> EmfColor {
  EmfColor {
    r: value.red,
    g: value.green,
    b: value.blue,
  }
}

fn decode_wmf_text(bytes: &[u8], char_set: u8) -> String {
  let bytes = bytes.iter().take_while(|byte| **byte != 0).copied();
  if char_set == crate::wmf::WmfCharacterSet::Symbol.raw() {
    // [MS-WMF] §2.3.3.2 assigns SYMBOL_CHARSET to code page 42. LibreOffice
    // represents its non-control bytes in the Symbol private-use range.
    return bytes
      .map(|byte| {
        if byte <= 0x1F {
          char::from(byte)
        } else {
          char::from_u32(0xF000 + u32::from(byte)).unwrap_or('\u{FFFD}')
        }
      })
      .collect();
  }
  let bytes = bytes.collect::<Vec<_>>();
  crate::string::SdkEncoding::WmfCharset(char_set)
    .decode(&bytes)
    .unwrap_or_else(|_| bytes.iter().map(|byte| char::from(*byte)).collect())
}

fn emf_current_font(state: &EmfVectorState) -> WmfTextFont {
  state
    .current_font
    .and_then(|id| state.fonts.get(&id))
    .map(|font| WmfTextFont {
      height: font.height,
      family: font.family.clone(),
      char_set: font.char_set,
      weight: font.weight,
      italic: font.italic,
      quality: font.quality,
    })
    .unwrap_or(WmfTextFont {
      height: 12,
      family: None,
      char_set: 0,
      weight: 400,
      italic: false,
      quality: crate::wmf::WmfFontQuality::Default.raw(),
    })
}

fn emf_arc_rect(data: &[u8], record_offset: usize) -> Result<(i32, i32, i32, i32), String> {
  Ok((
    read_i32(data, record_offset + 8)?,
    read_i32(data, record_offset + 12)?,
    read_i32(data, record_offset + 16)?,
    read_i32(data, record_offset + 20)?,
  ))
}

fn angle_from_emf_arc_point(rect: (i32, i32, i32, i32), x: i32, y: i32) -> f32 {
  let (left, top, right, bottom) = rect;
  let cx = (left + right) as f32 / 2.0;
  let cy = (top + bottom) as f32 / 2.0;
  (y as f32 - cy).atan2(x as f32 - cx).to_degrees()
}

fn sweep_from_emf_arc_points(
  data: &[u8],
  record_offset: usize,
  rect: (i32, i32, i32, i32),
) -> Result<f32, String> {
  let start = angle_from_emf_arc_point(
    rect,
    read_i32(data, record_offset + 24)?,
    read_i32(data, record_offset + 28)?,
  );
  let end = angle_from_emf_arc_point(
    rect,
    read_i32(data, record_offset + 32)?,
    read_i32(data, record_offset + 36)?,
  );
  let mut sweep = end - start;
  if sweep <= 0.0 {
    sweep += 360.0;
  }
  Ok(sweep)
}

fn angle_from_arc_point(value: crate::wmf::WmfArcRecord, x: i16, y: i16) -> f32 {
  let cx = (i32::from(value.left) + i32::from(value.right)) as f32 / 2.0;
  let cy = (i32::from(value.top) + i32::from(value.bottom)) as f32 / 2.0;
  (f32::from(y) - cy).atan2(f32::from(x) - cx).to_degrees()
}

fn sweep_from_arc_points(value: crate::wmf::WmfArcRecord) -> f32 {
  let start = angle_from_arc_point(value, value.x_radial_1, value.y_radial_1);
  let end = angle_from_arc_point(value, value.x_radial_2, value.y_radial_2);
  let mut sweep = end - start;
  if sweep <= 0.0 {
    sweep += 360.0;
  }
  sweep
}

fn decoded_raster_to_rgb(raster: &DecodedMetafile) -> Result<Option<RasterPixels>, String> {
  match raster.content_type {
    "image/png" => {
      let image = image::load_from_memory_with_format(&raster.data, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = image.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: image.into_raw(),
      }))
    }
    "image/jpeg" => Ok(None),
    _ => Ok(None),
  }
}

fn decoded_png_to_rgb(raster: &DecodedMetafile) -> Result<RasterPixels, String> {
  decoded_raster_to_rgb(raster)?
    .ok_or_else(|| "metafile transparent replay did not produce a PNG raster".to_string())
}

fn straight_rgba_from_black_white(black: &[u8], white: &[u8]) -> Result<Vec<u8>, String> {
  if black.len() != white.len() || !black.len().is_multiple_of(RGB_BYTES_PER_PIXEL) {
    return Err("metafile black/white replay buffers have incompatible lengths".to_string());
  }

  let mut rgba = Vec::with_capacity(black.len() / RGB_BYTES_PER_PIXEL * BGRA_BYTES_PER_PIXEL);
  for (black, white) in black
    .chunks_exact(RGB_BYTES_PER_PIXEL)
    .zip(white.chunks_exact(RGB_BYTES_PER_PIXEL))
  {
    let uncovered = white
      .iter()
      .zip(black)
      .map(|(white, black)| white.saturating_sub(*black))
      .max()
      .unwrap_or(u8::MAX);
    let alpha = u8::MAX - uncovered;
    if alpha == 0 {
      rgba.extend_from_slice(&[0, 0, 0, 0]);
      continue;
    }
    for channel in black {
      let straight =
        (u32::from(*channel) * u32::from(u8::MAX) + u32::from(alpha) / 2) / u32::from(alpha);
      rgba.push(straight.min(u32::from(u8::MAX)) as u8);
    }
    rgba.push(alpha);
  }
  Ok(rgba)
}

fn straight_rgba_from_black_white_with_mask(
  color_black: &[u8],
  color_white: &[u8],
  mask_black: &[u8],
  mask_white: &[u8],
) -> Result<Vec<u8>, String> {
  if color_black.len() != mask_black.len() || color_white.len() != mask_white.len() {
    return Err("metafile color and monochrome replay buffers have incompatible lengths".into());
  }
  let color = straight_rgba_from_black_white(color_black, color_white)?;
  let mask = straight_rgba_from_black_white(mask_black, mask_white)?;
  let mut rgba = Vec::with_capacity(color.len());
  for (color, mask) in color
    .chunks_exact(BGRA_BYTES_PER_PIXEL)
    .zip(mask.chunks_exact(BGRA_BYTES_PER_PIXEL))
  {
    let alpha = mask[3];
    if alpha == 0 {
      rgba.extend_from_slice(&[0, 0, 0, 0]);
    } else if color[3] != 0 {
      rgba.extend_from_slice(&[color[0], color[1], color[2], alpha]);
    } else {
      // A strongly hinted monochrome outline can cover a pixel that the
      // smooth color pass misses. Use the monochrome pass's source color for
      // that pixel instead of inventing color from a transparent sample.
      rgba.extend_from_slice(&[mask[0], mask[1], mask[2], alpha]);
    }
  }
  Ok(rgba)
}

fn straight_rgba_with_binary_coverage(
  color_black: &[u8],
  color_white: &[u8],
  mask_black: &[u8],
  mask_white: &[u8],
) -> Result<Vec<u8>, String> {
  if color_black.len() != color_white.len()
    || color_black.len() != mask_black.len()
    || color_white.len() != mask_white.len()
    || !color_black.len().is_multiple_of(RGB_BYTES_PER_PIXEL)
  {
    return Err("metafile color and monochrome replay buffers have incompatible lengths".into());
  }
  let mut rgba = Vec::with_capacity(color_black.len() / RGB_BYTES_PER_PIXEL * BGRA_BYTES_PER_PIXEL);
  for (((color_black, color_white), mask_black), mask_white) in color_black
    .chunks_exact(RGB_BYTES_PER_PIXEL)
    .zip(color_white.chunks_exact(RGB_BYTES_PER_PIXEL))
    .zip(mask_black.chunks_exact(RGB_BYTES_PER_PIXEL))
    .zip(mask_white.chunks_exact(RGB_BYTES_PER_PIXEL))
  {
    // The paired OLE replacement bitmap owns a one-bit destination mask.
    // ClearType coverage is independent per RGB stripe, so a pixel is
    // covered when any black/white-matte channel differs from the untouched
    // 0/255 background pair. Reducing the three stripes to the scalar alpha
    // used by ordinary transparent images would discard edge pixels whenever
    // one stripe remains untouched.
    let color_covered = color_black
      .iter()
      .zip(color_white)
      .any(|(black, white)| white.saturating_sub(*black) != u8::MAX);
    let mask_covered = mask_black
      .iter()
      .zip(mask_white)
      .any(|(black, white)| white.saturating_sub(*black) != u8::MAX);
    if !color_covered && !mask_covered {
      rgba.extend_from_slice(&[0, 0, 0, 0]);
    } else if color_covered {
      // Office stores the color replay over its black matte verbatim and
      // attaches the binary replacement mask separately. This preserves the
      // realized ClearType stripe values instead of unpremultiplying them as
      // an ordinary soft-alpha image.
      rgba.extend_from_slice(&[color_black[0], color_black[1], color_black[2], u8::MAX]);
    } else {
      rgba.extend_from_slice(&[mask_black[0], mask_black[1], mask_black[2], u8::MAX]);
    }
  }
  Ok(rgba)
}

fn bitmap16_to_rgb(data: &[u8]) -> Result<Option<RasterPixels>, String> {
  let bitmap = crate::wmf::WmfBitmap16::read_from_slice(data).map_err(|err| err.to_string())?;
  let width = bitmap.header.width.max(1) as usize;
  let height = bitmap.header.height.max(1) as usize;
  let bits_pixel = bitmap.header.bits_pixel;
  let stride = bitmap
    .header
    .computed_width_bytes()
    .map_err(|err| err.to_string())?;
  let required = stride
    .checked_mul(height)
    .ok_or_else(|| "Bitmap16 dimensions overflow".to_string())?;
  if bitmap.bits.len() < required {
    return Err("Bitmap16 bits are truncated".to_string());
  }
  let mut rgb = vec![0u8; width * height * RGB_BYTES_PER_PIXEL];
  for row in 0..height {
    let src_row = height - 1 - row;
    let src = &bitmap.bits[src_row * stride..src_row * stride + stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    match bits_pixel {
      1 => {
        for col in 0..width {
          let bit = (src[col / 8] >> (7 - (col % 8))) & 1;
          let value = if bit == 0 { 0 } else { 255 };
          let offset = col * RGB_BYTES_PER_PIXEL;
          dest[offset] = value;
          dest[offset + 1] = value;
          dest[offset + 2] = value;
        }
      }
      8 => {
        for (col, value) in src.iter().copied().enumerate().take(width) {
          let offset = col * RGB_BYTES_PER_PIXEL;
          dest[offset] = value;
          dest[offset + 1] = value;
          dest[offset + 2] = value;
        }
      }
      24 => {
        for col in 0..width {
          let src_pixel =
            &src[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      _ => return Ok(None),
    }
  }
  Ok(Some(RasterPixels { width, height, rgb }))
}

fn packed_dib_to_rgb(
  data: &[u8],
  color_usage: DibColorUsage,
) -> Result<Option<RasterPixels>, String> {
  packed_dib_to_rgb_with_palette_override(data, color_usage, None)
}

fn packed_dib_to_rgb_with_palette_override(
  data: &[u8],
  color_usage: DibColorUsage,
  monochrome_palette_override: Option<[[u8; 3]; 2]>,
) -> Result<Option<RasterPixels>, String> {
  let dib =
    DeviceIndependentBitmap::from_packed_slice(data, color_usage).map_err(|err| err.to_string())?;
  device_independent_bitmap_to_rgb(&dib, color_usage, monochrome_palette_override)
}

fn device_independent_bitmap_to_rgb(
  dib: &DeviceIndependentBitmap,
  color_usage: DibColorUsage,
  monochrome_palette_override: Option<[[u8; 3]; 2]>,
) -> Result<Option<RasterPixels>, String> {
  match dib.info.header.compression_kind() {
    Some(BitmapCompression::Png) => {
      let image = image::load_from_memory_with_format(&dib.bits, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?
        .to_rgb8();
      let (width, height) = image.dimensions();
      Ok(Some(RasterPixels {
        width: width as usize,
        height: height as usize,
        rgb: image.into_raw(),
      }))
    }
    Some(BitmapCompression::Jpeg) => Ok(None),
    Some(BitmapCompression::Rgb) => dib_rgb_bits_to_rgb(
      &dib.info.header,
      &dib.bits,
      &dib.info,
      color_usage,
      monochrome_palette_override,
    )
    .map(Some),
    Some(BitmapCompression::Bitfields) => {
      dib_bitfields_to_rgb(&dib.info.header, &dib.bits, &dib.info).map(Some)
    }
    Some(BitmapCompression::Rle8) | Some(BitmapCompression::Rle4) => {
      dib_rle_to_rgb(&dib.info.header, &dib.bits, &dib.info, color_usage).map(Some)
    }
    _ => Ok(None),
  }
}

fn dib_rgb_bits_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
  color_usage: DibColorUsage,
  monochrome_palette_override: Option<[[u8; 3]; 2]>,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let bit_count = header.bit_count();
  let row_stride = header
    .scan_line_stride_bytes()
    .map_err(|err| err.to_string())? as usize;
  let required_size = row_stride
    .checked_mul(height_abs)
    .ok_or_else(|| "DIB dimensions overflow".to_string())?;
  if bits.len() < required_size {
    return Err(format!(
      "DIB payload is truncated: need {required_size} bytes, got {}",
      bits.len()
    ));
  }
  let mut palette = match bit_count {
    1 | 4 | 8 => match info
      .parse_color_table(color_usage)
      .map_err(|err| err.to_string())?
    {
      DibColorTable::RgbQuads { entries, .. } => entries,
      _ => Vec::new(),
    },
    _ => Vec::new(),
  };
  if bit_count == 1
    && let Some(colors) = monochrome_palette_override
  {
    palette = colors
      .map(|[red, green, blue]| crate::RgbQuad {
        blue,
        green,
        red,
        reserved: 0,
      })
      .to_vec();
  }
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let src = &bits[src_row * row_stride..src_row * row_stride + row_stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    match bit_count {
      1 => {
        for col in 0..width {
          let byte = src[col / 8];
          let index = ((byte >> (7 - (col % 8))) & 0x01) as usize;
          write_palette_pixel(dest, col, &palette, index);
        }
      }
      4 => {
        for col in 0..width {
          let byte = src[col / 2];
          let index = if col.is_multiple_of(2) {
            (byte >> 4) as usize
          } else {
            (byte & 0x0f) as usize
          };
          write_palette_pixel(dest, col, &palette, index);
        }
      }
      8 => {
        for (col, index) in src.iter().copied().enumerate().take(width) {
          write_palette_pixel(dest, col, &palette, index as usize);
        }
      }
      24 => {
        for col in 0..width {
          let src_pixel =
            &src[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      32 => {
        for col in 0..width {
          let src_pixel =
            &src[col * BGRA_BYTES_PER_PIXEL..col * BGRA_BYTES_PER_PIXEL + BGRA_BYTES_PER_PIXEL];
          let dest_pixel =
            &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
          dest_pixel[0] = src_pixel[2];
          dest_pixel[1] = src_pixel[1];
          dest_pixel[2] = src_pixel[0];
        }
      }
      other => return Err(format!("unsupported BI_RGB bit depth: {other}")),
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn dib_bitfields_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let bit_count = header.bit_count();
  let bytes_per_pixel = match bit_count {
    16 => 2,
    32 => 4,
    other => return Err(format!("unsupported BI_BITFIELDS bit depth: {other}")),
  };
  let row_stride = header
    .scan_line_stride_bytes()
    .map_err(|err| err.to_string())? as usize;
  let required_size = row_stride
    .checked_mul(height_abs)
    .ok_or_else(|| "DIB dimensions overflow".to_string())?;
  if bits.len() < required_size {
    return Err(format!(
      "DIB payload is truncated: need {required_size} bytes, got {}",
      bits.len()
    ));
  }
  let masks = info.bitfield_masks().map_err(|err| err.to_string())?;
  let masks = masks.unwrap_or(match bit_count {
    16 => [0x7C00, 0x03E0, 0x001F],
    32 => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF],
    _ => unreachable!(),
  });
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let src = &bits[src_row * row_stride..src_row * row_stride + row_stride];
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      let offset = col * bytes_per_pixel;
      let value = if bytes_per_pixel == 2 {
        u32::from(u16::from_le_bytes([src[offset], src[offset + 1]]))
      } else {
        u32::from_le_bytes([
          src[offset],
          src[offset + 1],
          src[offset + 2],
          src[offset + 3],
        ])
      };
      let dest_pixel =
        &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
      dest_pixel[0] = bitfield_channel(value, masks[0]);
      dest_pixel[1] = bitfield_channel(value, masks[1]);
      dest_pixel[2] = bitfield_channel(value, masks[2]);
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn bitfield_channel(value: u32, mask: u32) -> u8 {
  if mask == 0 {
    return 0;
  }
  let shift = mask.trailing_zeros();
  let bits = mask.count_ones();
  let raw = (value & mask) >> shift;
  let max = (1u32 << bits) - 1;
  ((raw * 255 + max / 2) / max) as u8
}

fn dib_rle_to_rgb(
  header: &DibHeader,
  bits: &[u8],
  info: &crate::DibBitmapInfo,
  color_usage: DibColorUsage,
) -> Result<RasterPixels, String> {
  let width = header.width();
  let height = header.height();
  if width <= 0 || height == 0 {
    return Err(format!("unsupported DIB size {width}x{height}"));
  }
  let width = width as usize;
  let height_abs = header.height_abs() as usize;
  let palette = match info
    .parse_color_table(color_usage)
    .map_err(|err| err.to_string())?
  {
    DibColorTable::RgbQuads { entries, .. } => entries,
    _ => Vec::new(),
  };
  let indices = match header.compression_kind() {
    Some(BitmapCompression::Rle8) => decode_rle8_indices(bits, width, height_abs)?,
    Some(BitmapCompression::Rle4) => decode_rle4_indices(bits, width, height_abs)?,
    other => return Err(format!("unsupported RLE compression: {other:?}")),
  };
  let mut rgb = vec![0u8; width * height_abs * RGB_BYTES_PER_PIXEL];
  for row in 0..height_abs {
    let src_row = if header.is_top_down() {
      row
    } else {
      height_abs - 1 - row
    };
    let dest = &mut rgb[row * width * RGB_BYTES_PER_PIXEL..(row + 1) * width * RGB_BYTES_PER_PIXEL];
    for col in 0..width {
      write_palette_pixel(dest, col, &palette, indices[src_row * width + col] as usize);
    }
  }
  Ok(RasterPixels {
    width,
    height: height_abs,
    rgb,
  })
}

fn decode_rle8_indices(bits: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
  let mut out = vec![0u8; width * height];
  let mut x = 0usize;
  let mut y = 0usize;
  let mut pos = 0usize;
  while pos + 1 < bits.len() && y < height {
    let count = bits[pos];
    let value = bits[pos + 1];
    pos += 2;
    if count != 0 {
      for _ in 0..count {
        if x < width && y < height {
          out[y * width + x] = value;
        }
        x = x.saturating_add(1);
      }
      continue;
    }
    match value {
      0 => {
        x = 0;
        y = y.saturating_add(1);
      }
      1 => break,
      2 if pos + 1 < bits.len() => {
        x = x.saturating_add(bits[pos] as usize);
        y = y.saturating_add(bits[pos + 1] as usize);
        pos += 2;
      }
      n => {
        let n = n as usize;
        if pos + n > bits.len() {
          return Err("RLE8 absolute run is truncated".to_string());
        }
        for value in &bits[pos..pos + n] {
          if x < width && y < height {
            out[y * width + x] = *value;
          }
          x = x.saturating_add(1);
        }
        pos += n + (n % 2);
      }
    }
  }
  Ok(out)
}

fn decode_rle4_indices(bits: &[u8], width: usize, height: usize) -> Result<Vec<u8>, String> {
  let mut out = vec![0u8; width * height];
  let mut x = 0usize;
  let mut y = 0usize;
  let mut pos = 0usize;
  while pos + 1 < bits.len() && y < height {
    let count = bits[pos];
    let value = bits[pos + 1];
    pos += 2;
    if count != 0 {
      let high = value >> 4;
      let low = value & 0x0F;
      for index in 0..count as usize {
        if x < width && y < height {
          out[y * width + x] = if index.is_multiple_of(2) { high } else { low };
        }
        x = x.saturating_add(1);
      }
      continue;
    }
    match value {
      0 => {
        x = 0;
        y = y.saturating_add(1);
      }
      1 => break,
      2 if pos + 1 < bits.len() => {
        x = x.saturating_add(bits[pos] as usize);
        y = y.saturating_add(bits[pos + 1] as usize);
        pos += 2;
      }
      n => {
        let pixel_count = n as usize;
        let byte_count = pixel_count.div_ceil(2);
        if pos + byte_count > bits.len() {
          return Err("RLE4 absolute run is truncated".to_string());
        }
        for index in 0..pixel_count {
          let byte = bits[pos + index / 2];
          let value = if index.is_multiple_of(2) {
            byte >> 4
          } else {
            byte & 0x0F
          };
          if x < width && y < height {
            out[y * width + x] = value;
          }
          x = x.saturating_add(1);
        }
        pos += byte_count + (byte_count % 2);
      }
    }
  }
  Ok(out)
}

fn write_palette_pixel(dest: &mut [u8], col: usize, palette: &[crate::RgbQuad], index: usize) {
  let Some(color) = palette.get(index) else {
    return;
  };
  let dest_pixel =
    &mut dest[col * RGB_BYTES_PER_PIXEL..col * RGB_BYTES_PER_PIXEL + RGB_BYTES_PER_PIXEL];
  dest_pixel[0] = color.red;
  dest_pixel[1] = color.green;
  dest_pixel[2] = color.blue;
}

fn draw_glyph_5x7(
  state: &mut EmfVectorState,
  x: i32,
  y: i32,
  ch: char,
  color: EmfColor,
  scale: usize,
) {
  let glyph = glyph_5x7(ch);
  for (row, bits) in glyph.iter().copied().enumerate() {
    for col in 0..5 {
      if bits & (1 << (4 - col)) == 0 {
        continue;
      }
      for yy in 0..scale {
        for xx in 0..scale {
          state.set_vector_pixel(
            x + (col * scale + xx) as i32,
            y + (row * scale + yy) as i32,
            color,
          );
        }
      }
    }
  }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
  match ch.to_ascii_uppercase() {
    '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
    '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
    '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
    '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
    '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
    '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
    '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
    '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
    '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
    '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
    'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
    'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
    'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
    'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
    'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
    'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
    'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
    'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
    'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
    'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
    'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
    'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
    'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
    'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
    'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
    'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
    'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
    'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
    'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
    'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
    'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
    'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
    'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
    'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
    '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
    '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
    ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
    ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
    '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
    _ => [0x1F, 0x11, 0x15, 0x15, 0x15, 0x11, 0x1F],
  }
}

fn clamp_canvas_size(width: usize, height: usize, max_pixels: Option<u32>) -> (usize, usize) {
  let max_pixels = max_pixels.unwrap_or(DEFAULT_MAX_PIXELS as u32).max(1) as usize;
  match width.checked_mul(height) {
    Some(pixels) if pixels <= max_pixels => (width, height),
    Some(pixels) => {
      let scale = (max_pixels as f64 / pixels as f64).sqrt();
      (
        ((width as f64 * scale).round() as usize).max(1),
        ((height as f64 * scale).round() as usize).max(1),
      )
    }
    None => (DEFAULT_RENDER_WIDTH, DEFAULT_RENDER_HEIGHT),
  }
}

fn visit_polygon_scanline_spans(
  points: &[(f32, f32)],
  width: usize,
  height: usize,
  mut visit: impl FnMut(usize, usize, usize),
) {
  if points.len() < 3 || width == 0 || height == 0 {
    return;
  }

  let mut min_y = f32::INFINITY;
  let mut max_y = f32::NEG_INFINITY;
  for &(_, y) in points {
    if y.is_finite() {
      min_y = min_y.min(y);
      max_y = max_y.max(y);
    }
  }
  if !min_y.is_finite() || !max_y.is_finite() {
    return;
  }

  // A scanline samples at y + 0.5, so rows wholly outside the polygon's
  // vertical bounds cannot contribute. Retaining one floor/ceil boundary
  // row preserves the existing edge rule while avoiding a full-canvas scan
  // for every small metafile polygon.
  let start_y = min_y.floor().max(0.0).min(height as f32) as usize;
  let end_y = max_y.ceil().max(0.0).min(height as f32) as usize;
  let mut intersections = Vec::new();
  for y in start_y..end_y {
    let scan_y = y as f32 + 0.5;
    intersections.clear();
    for index in 0..points.len() {
      let (x1, y1) = points[index];
      let (x2, y2) = points[(index + 1) % points.len()];
      if (y1 <= scan_y && y2 > scan_y) || (y2 <= scan_y && y1 > scan_y) {
        let t = (scan_y - y1) / (y2 - y1);
        intersections.push(x1 + t * (x2 - x1));
      }
    }
    intersections.sort_by(|a, b| a.total_cmp(b));
    for pair in intersections.chunks_exact(2) {
      // Sample coverage at pixel centers and keep the trailing polygon edge
      // half-open. Adjacent polygons emitted for GDI gradients share that
      // edge; rounding both intersections outward paints it twice, which is
      // visibly wrong under R2_XORPEN.
      let start_x = (pair[0] - 0.5).ceil().max(0.0).min(width as f32) as usize;
      let end_x = (pair[1] - 0.5).ceil().max(0.0).min(width as f32) as usize;
      if end_x > start_x {
        visit(y, start_x, end_x);
      }
    }
  }
}

fn axis_aligned_clip_rect(
  points: &[(f32, f32)],
  width: usize,
  height: usize,
) -> Option<(i32, i32, i32, i32)> {
  let points = if points.len() == 5 && points_approximately_equal(points[0], points[4]) {
    &points[..4]
  } else {
    points
  };
  if points.len() != 4 || points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
    return None;
  }
  for index in 0..points.len() {
    let current = points[index];
    let next = points[(index + 1) % points.len()];
    if !approximately_equal(current.0, next.0) && !approximately_equal(current.1, next.1) {
      return None;
    }
  }

  let min_x = points
    .iter()
    .map(|point| point.0)
    .fold(f32::INFINITY, f32::min);
  let max_x = points
    .iter()
    .map(|point| point.0)
    .fold(f32::NEG_INFINITY, f32::max);
  let min_y = points
    .iter()
    .map(|point| point.1)
    .fold(f32::INFINITY, f32::min);
  let max_y = points
    .iter()
    .map(|point| point.1)
    .fold(f32::NEG_INFINITY, f32::max);
  Some((
    min_x.floor().max(0.0).min(width as f32) as i32,
    (min_y - 0.5).ceil().max(0.0).min(height as f32) as i32,
    max_x.ceil().max(0.0).min(width as f32) as i32,
    (max_y - 0.5).ceil().max(0.0).min(height as f32) as i32,
  ))
}

fn points_approximately_equal(left: (f32, f32), right: (f32, f32)) -> bool {
  approximately_equal(left.0, right.0) && approximately_equal(left.1, right.1)
}

fn approximately_equal(left: f32, right: f32) -> bool {
  (left - right).abs() <= 0.001
}

fn intersect_rects(
  left: (i32, i32, i32, i32),
  right: (i32, i32, i32, i32),
) -> (i32, i32, i32, i32) {
  let x1 = left.0.max(right.0);
  let y1 = left.1.max(right.1);
  let x2 = left.2.min(right.2).max(x1);
  let y2 = left.3.min(right.3).max(y1);
  (x1, y1, x2, y2)
}

fn clip_line_to_rect(
  start: (f64, f64),
  end: (f64, f64),
  rect: (f64, f64, f64, f64),
) -> Option<((f64, f64), (f64, f64))> {
  if ![
    start.0, start.1, end.0, end.1, rect.0, rect.1, rect.2, rect.3,
  ]
  .iter()
  .all(|value| value.is_finite())
    || rect.2 < rect.0
    || rect.3 < rect.1
  {
    return None;
  }

  let dx = end.0 - start.0;
  let dy = end.1 - start.1;
  let mut first: f64 = 0.0;
  let mut last: f64 = 1.0;
  for (direction, distance) in [
    (-dx, start.0 - rect.0),
    (dx, rect.2 - start.0),
    (-dy, start.1 - rect.1),
    (dy, rect.3 - start.1),
  ] {
    if direction == 0.0 {
      if distance < 0.0 {
        return None;
      }
      continue;
    }
    let ratio = distance / direction;
    if direction < 0.0 {
      first = first.max(ratio);
    } else {
      last = last.min(ratio);
    }
    if first > last {
      return None;
    }
  }

  Some((
    (start.0 + first * dx, start.1 + first * dy),
    (start.0 + last * dx, start.1 + last * dy),
  ))
}

fn read_poly_polygons_i32(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Result<Vec<Vec<EmfPoint>>, String> {
  let polygon_count = read_u32(data, record_offset + 24)? as usize;
  let total_points = read_u32(data, record_offset + 28)? as usize;
  let counts_offset = record_offset + 32;
  let points_offset = counts_offset
    .checked_add(polygon_count * 4)
    .ok_or_else(|| "EMF polygon counts overflow".to_string())?;
  if points_offset > record_offset + record_size {
    return Ok(Vec::new());
  }
  let mut counts = Vec::with_capacity(polygon_count);
  for index in 0..polygon_count {
    counts.push(read_u32(data, counts_offset + index * 4)? as usize);
  }
  let Some(points) = read_points_i32(data, points_offset, total_points) else {
    return Ok(Vec::new());
  };
  Ok(split_polygons(points, counts))
}

fn read_poly_polygons_i16(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Result<Vec<Vec<EmfPoint>>, String> {
  let polygon_count = read_u32(data, record_offset + 24)? as usize;
  let total_points = read_u32(data, record_offset + 28)? as usize;
  let counts_offset = record_offset + 32;
  let points_offset = counts_offset
    .checked_add(polygon_count * 4)
    .ok_or_else(|| "EMF polygon counts overflow".to_string())?;
  if points_offset > record_offset + record_size {
    return Ok(Vec::new());
  }
  let mut counts = Vec::with_capacity(polygon_count);
  for index in 0..polygon_count {
    counts.push(read_u32(data, counts_offset + index * 4)? as usize);
  }
  let Some(points) = read_points_i16(data, points_offset, total_points) else {
    return Ok(Vec::new());
  };
  Ok(split_polygons(points, counts))
}

fn split_polygons(points: Vec<EmfPoint>, counts: Vec<usize>) -> Vec<Vec<EmfPoint>> {
  let mut polygons = Vec::with_capacity(counts.len());
  let mut cursor = 0usize;
  for count in counts {
    let end = cursor.saturating_add(count).min(points.len());
    polygons.push(points[cursor..end].to_vec());
    cursor = end;
  }
  polygons
}

fn read_points_i32(data: &[u8], offset: usize, count: usize) -> Option<Vec<EmfPoint>> {
  let end = offset.checked_add(count.checked_mul(8)?)?;
  if end > data.len() {
    return None;
  }
  let mut points = Vec::with_capacity(count);
  for index in 0..count {
    let point_offset = offset + index * 8;
    points.push(EmfPoint {
      x: read_i32(data, point_offset).ok()?,
      y: read_i32(data, point_offset + 4).ok()?,
    });
  }
  Some(points)
}

fn read_points_i16(data: &[u8], offset: usize, count: usize) -> Option<Vec<EmfPoint>> {
  let end = offset.checked_add(count.checked_mul(4)?)?;
  if end > data.len() {
    return None;
  }
  let mut points = Vec::with_capacity(count);
  for index in 0..count {
    let point_offset = offset + index * 4;
    points.push(EmfPoint {
      x: i32::from(read_i16(data, point_offset).ok()?),
      y: i32::from(read_i16(data, point_offset + 2).ok()?),
    });
  }
  Some(points)
}

fn read_color_ref(data: &[u8], offset: usize) -> Result<EmfColor, String> {
  let color_ref = read_u32(data, offset)?;
  Ok(EmfColor {
    r: (color_ref & 0xff) as u8,
    g: ((color_ref >> 8) & 0xff) as u8,
    b: ((color_ref >> 16) & 0xff) as u8,
  })
}

fn read_xform(data: &[u8], offset: usize) -> Result<EmfTransform, String> {
  Ok(EmfTransform {
    m11: read_f32(data, offset)?,
    m12: read_f32(data, offset + 4)?,
    m21: read_f32(data, offset + 8)?,
    m22: read_f32(data, offset + 12)?,
    dx: read_f32(data, offset + 16)?,
    dy: read_f32(data, offset + 20)?,
  })
}

fn rgb_to_png(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
  let mut output = Vec::new();
  let encoder = PngEncoder::new(&mut output);
  encoder
    .write_image(rgb, width, height, ColorType::Rgb8.into())
    .map_err(|err| err.to_string())?;
  Ok(output)
}

fn rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
  let mut output = Vec::new();
  let encoder = PngEncoder::new(&mut output);
  encoder
    .write_image(rgba, width, height, ColorType::Rgba8.into())
    .map_err(|err| err.to_string())?;
  Ok(output)
}

fn emf_natural_canvas_size(data: &[u8]) -> Result<(usize, usize), String> {
  let bounds_width = i64::from(read_i32(data, EMF_BOUNDS_RIGHT_OFFSET)?)
    - i64::from(read_i32(data, EMF_BOUNDS_LEFT_OFFSET)?)
    + 1;
  let bounds_height = i64::from(read_i32(data, EMF_BOUNDS_BOTTOM_OFFSET)?)
    - i64::from(read_i32(data, EMF_BOUNDS_TOP_OFFSET)?)
    + 1;
  let fallback = (
    usize::try_from(bounds_width.max(1)).unwrap_or(1),
    usize::try_from(bounds_height.max(1)).unwrap_or(1),
  );

  let frame_width = (i64::from(read_i32(data, EMF_FRAME_RIGHT_OFFSET)?)
    - i64::from(read_i32(data, EMF_FRAME_LEFT_OFFSET)?))
  .unsigned_abs();
  let frame_height = (i64::from(read_i32(data, EMF_FRAME_BOTTOM_OFFSET)?)
    - i64::from(read_i32(data, EMF_FRAME_TOP_OFFSET)?))
  .unsigned_abs();
  let device_width = read_i32(data, EMF_DEVICE_WIDTH_OFFSET)?.unsigned_abs();
  let device_height = read_i32(data, EMF_DEVICE_HEIGHT_OFFSET)?.unsigned_abs();
  let millimeters_width = read_i32(data, EMF_MILLIMETERS_WIDTH_OFFSET)?.unsigned_abs();
  let millimeters_height = read_i32(data, EMF_MILLIMETERS_HEIGHT_OFFSET)?.unsigned_abs();
  if frame_width == 0
    || frame_height == 0
    || device_width == 0
    || device_height == 0
    || millimeters_width == 0
    || millimeters_height == 0
  {
    return Ok(fallback);
  }

  // [MS-EMF] Header.Frame is in 0.01 mm. Device and Millimeters describe
  // the reference device, so together they recover the authored playback
  // surface in device pixels. Bounds encloses only the marks and must not be
  // used to crop away the surrounding metafile surface.
  let pixel_axis = |frame: u64, device: u32, millimeters: u32| {
    ((frame as f64 * f64::from(device)) / (f64::from(millimeters) * 100.0))
      .round()
      .max(1.0) as usize
  };
  Ok((
    pixel_axis(frame_width, device_width, millimeters_width),
    pixel_axis(frame_height, device_height, millimeters_height),
  ))
}

#[derive(Clone, Copy, Debug)]
struct EmfPlaybackGeometry {
  width: usize,
  height: usize,
  origin_x: f32,
  origin_y: f32,
  scale_x: f32,
  scale_y: f32,
}

fn emf_playback_geometry(data: &[u8]) -> Result<EmfPlaybackGeometry, String> {
  let (width, height) = emf_natural_canvas_size(data)?;
  let bounds_origin = (
    read_i32(data, EMF_BOUNDS_LEFT_OFFSET)? as f32,
    read_i32(data, EMF_BOUNDS_TOP_OFFSET)? as f32,
  );
  let frame_left = read_i32(data, EMF_FRAME_LEFT_OFFSET)?;
  let frame_top = read_i32(data, EMF_FRAME_TOP_OFFSET)?;
  let frame_right = read_i32(data, EMF_FRAME_RIGHT_OFFSET)?;
  let frame_bottom = read_i32(data, EMF_FRAME_BOTTOM_OFFSET)?;
  let device_width = read_i32(data, EMF_DEVICE_WIDTH_OFFSET)?.unsigned_abs();
  let device_height = read_i32(data, EMF_DEVICE_HEIGHT_OFFSET)?.unsigned_abs();
  let millimeters_width = read_i32(data, EMF_MILLIMETERS_WIDTH_OFFSET)?.unsigned_abs();
  let millimeters_height = read_i32(data, EMF_MILLIMETERS_HEIGHT_OFFSET)?.unsigned_abs();
  if frame_left == frame_right
    || frame_top == frame_bottom
    || device_width == 0
    || device_height == 0
    || millimeters_width == 0
    || millimeters_height == 0
  {
    // Header.Bounds is only a last-resort playback surface when the physical
    // Frame/reference-device tuple is unavailable. In that fallback case its
    // nonzero origin must still map to the first destination pixel.
    return Ok(EmfPlaybackGeometry {
      width,
      height,
      origin_x: bounds_origin.0,
      origin_y: bounds_origin.1,
      scale_x: 1.0,
      scale_y: 1.0,
    });
  }

  let device_coordinate = |frame: i32, device: u32, millimeters: u32| {
    frame as f64 * f64::from(device) / (f64::from(millimeters) * 100.0)
  };
  let origin_x = device_coordinate(frame_left, device_width, millimeters_width);
  let origin_y = device_coordinate(frame_top, device_height, millimeters_height);
  let extent_x = device_coordinate(
    frame_right.saturating_sub(frame_left),
    device_width,
    millimeters_width,
  );
  let extent_y = device_coordinate(
    frame_bottom.saturating_sub(frame_top),
    device_height,
    millimeters_height,
  );

  // PlayEnhMetaFile maps Header.Frame, expressed in 0.01 mm, onto the caller's
  // destination rectangle. Keep that outer playback transform separate from
  // the recorded world/page transforms. Header.Bounds encloses ink and is not
  // a substitute for this translation (Wine enhmetafile.c; LibreOffice
  // EmfPlusHelperData::mappingChanged).
  Ok(EmfPlaybackGeometry {
    width,
    height,
    origin_x: origin_x as f32,
    origin_y: origin_y as f32,
    scale_x: (width as f64 / extent_x) as f32,
    scale_y: (height as f64 / extent_y) as f32,
  })
}

fn emf_gdiplus_playback_geometry(data: &[u8]) -> Result<EmfPlaybackGeometry, String> {
  let fallback = emf_playback_geometry(data)?;
  let frame_left = read_i32(data, EMF_FRAME_LEFT_OFFSET)?;
  let frame_top = read_i32(data, EMF_FRAME_TOP_OFFSET)?;
  let frame_right = read_i32(data, EMF_FRAME_RIGHT_OFFSET)?;
  let frame_bottom = read_i32(data, EMF_FRAME_BOTTOM_OFFSET)?;
  let device_width = read_i32(data, EMF_DEVICE_WIDTH_OFFSET)?.unsigned_abs();
  let device_height = read_i32(data, EMF_DEVICE_HEIGHT_OFFSET)?.unsigned_abs();
  let millimeters_width = read_i32(data, EMF_MILLIMETERS_WIDTH_OFFSET)?.unsigned_abs();
  let millimeters_height = read_i32(data, EMF_MILLIMETERS_HEIGHT_OFFSET)?.unsigned_abs();
  if frame_left == frame_right
    || frame_top == frame_bottom
    || device_width == 0
    || device_height == 0
    || millimeters_width == 0
    || millimeters_height == 0
  {
    return Ok(fallback);
  }

  let device_coordinate = |frame: i64, device: u32, millimeters: u32| {
    frame as f64 * f64::from(device) / (f64::from(millimeters) * 100.0)
  };
  let origin_x = device_coordinate(i64::from(frame_left), device_width, millimeters_width).round();
  let origin_y = device_coordinate(i64::from(frame_top), device_height, millimeters_height).round();
  let width = device_coordinate(
    i64::from(frame_right) - i64::from(frame_left),
    device_width,
    millimeters_width,
  )
  .abs()
  .round()
  .max(0.0) as usize
    + 1;
  let height = device_coordinate(
    i64::from(frame_bottom) - i64::from(frame_top),
    device_height,
    millimeters_height,
  )
  .abs()
  .round()
  .max(0.0) as usize
    + 1;

  // GDI+ exposes Header.X/Y as the rounded reference-device frame origin and
  // Header.Width/Height as the rounded frame extent plus one: the EMF frame
  // is inclusive. Office's PDF metafile Forms retain that coordinate surface.
  // This is distinct from PlayEnhMetaFile's outer destination transform used
  // by raster replay. See libgdiplus/src/metafile.c:gdip_read_emf_header().
  Ok(EmfPlaybackGeometry {
    width,
    height,
    origin_x: origin_x as f32,
    origin_y: origin_y as f32,
    scale_x: 1.0,
    scale_y: 1.0,
  })
}

fn emf_plus_units_to_device_scale(
  unit: EmfPlusUnitType,
  page_scale: f32,
  logical_dpi: f32,
  video_display: bool,
) -> f32 {
  let dpi = logical_dpi.max(1.0);
  let units_to_pixels = match unit {
    EmfPlusUnitType::World | EmfPlusUnitType::Pixel => 1.0,
    EmfPlusUnitType::Display if video_display => 1.0,
    EmfPlusUnitType::Display => dpi / 100.0,
    EmfPlusUnitType::Point => dpi / 72.0,
    EmfPlusUnitType::Inch => dpi,
    EmfPlusUnitType::Document => dpi / 300.0,
    EmfPlusUnitType::Millimeter => dpi / 25.4,
  };
  if unit == EmfPlusUnitType::Display {
    // GDI+ ignores PageScale for UnitDisplay. Windows does not emit Display
    // or World in SetPageTransform, but accepting their playback semantics is
    // useful for third-party producers.
    units_to_pixels
  } else {
    units_to_pixels * page_scale
  }
}

fn emf_physical_size(data: &[u8]) -> Option<MetafilePhysicalSize> {
  const HUNDREDTHS_OF_MILLIMETER_PER_INCH: f32 = 2_540.0;
  let frame_width = (i64::from(read_i32(data, EMF_FRAME_RIGHT_OFFSET).ok()?)
    - i64::from(read_i32(data, EMF_FRAME_LEFT_OFFSET).ok()?))
  .unsigned_abs();
  let frame_height = (i64::from(read_i32(data, EMF_FRAME_BOTTOM_OFFSET).ok()?)
    - i64::from(read_i32(data, EMF_FRAME_TOP_OFFSET).ok()?))
  .unsigned_abs();
  if frame_width == 0 || frame_height == 0 {
    return None;
  }
  let (natural_width_px, natural_height_px) = emf_natural_canvas_size(data).ok()?;
  Some(MetafilePhysicalSize {
    width_pt: frame_width as f32 * 72.0 / HUNDREDTHS_OF_MILLIMETER_PER_INCH,
    height_pt: frame_height as f32 * 72.0 / HUNDREDTHS_OF_MILLIMETER_PER_INCH,
    natural_width_px: u32::try_from(natural_width_px).ok()?,
    natural_height_px: u32::try_from(natural_height_px).ok()?,
  })
}

fn is_emf(data: &[u8]) -> bool {
  emf_header_record_size(data).is_some()
}

fn emf_header_record_size(data: &[u8]) -> Option<usize> {
  if !crate::emf::looks_like_emf(data) {
    return None;
  }
  let size = read_u32(data, 4).ok()? as usize;
  (size >= crate::emf::EMF_HEADER_MIN_SIZE as usize && size.is_multiple_of(4) && size <= data.len())
    .then_some(size)
}

fn extract_emr_ext_text_out_w(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<String> {
  let units = emr_ext_text_out_w_units(data, record_offset, record_size)?;
  Some(
    String::from_utf16_lossy(&units)
      .trim_end_matches('\0')
      .to_string(),
  )
}

fn extract_semantic_emr_ext_text_out_w(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<String> {
  let units = emr_ext_text_out_w_units(data, record_offset, record_size)?;
  // [MS-EMF] §2.3.5.8 defines each EMR_EXTTEXTOUTW record as an
  // independent UTF-16LE Unicode string. A lone surrogate can still paint a
  // GDI missing-glyph cell, but it has no Unicode scalar value and therefore
  // must not become U+FFFD in searchable semantic text. Raster replay keeps
  // using the lossy decoder above so the visible glyph cell is preserved.
  let text = char::decode_utf16(units)
    .filter_map(|character| character.ok())
    .collect::<String>();
  Some(text.trim_end_matches('\0').to_string())
}

fn emr_ext_text_out_w_units(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<Vec<u16>> {
  let text = ext_text_record(data, record_offset, record_size)?;
  let byte_len = text.characters.checked_mul(2)?;
  let start = record_offset.checked_add(text.string_offset)?;
  let end = start.checked_add(byte_len)?;
  let bytes = data.get(start..end)?;
  let units = bytes
    .chunks_exact(2)
    .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
    .collect::<Vec<_>>();
  Some(units)
}

fn extract_emr_ext_text_out_a(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<String> {
  let text = ext_text_record(data, record_offset, record_size)?;
  let start = record_offset.checked_add(text.string_offset)?;
  let end = start.checked_add(text.characters)?;
  let bytes = data.get(start..end)?;
  Some(
    bytes
      .iter()
      .take_while(|byte| **byte != 0)
      .map(|byte| char::from(*byte))
      .collect(),
  )
}

#[derive(Clone, Copy, Debug)]
struct ExtTextRecord {
  graphics_mode: u32,
  x_scale: f32,
  y_scale: f32,
  x: i32,
  y: i32,
  characters: usize,
  string_offset: usize,
  options: u32,
  dx_offset: Option<usize>,
}

fn ext_text_record(data: &[u8], record_offset: usize, record_size: usize) -> Option<ExtTextRecord> {
  // with rclBounds, graphics mode, scales, then EMRTEXT. EMRTEXT::offString is
  // relative to the record start.
  const EMRTEXT_OFFSET: usize = 36;
  const GRAPHICS_MODE_OFFSET: usize = 24;
  const X_SCALE_OFFSET: usize = 28;
  const Y_SCALE_OFFSET: usize = 32;
  const EMRTEXT_REFERENCE_X_OFFSET: usize = EMRTEXT_OFFSET;
  const EMRTEXT_REFERENCE_Y_OFFSET: usize = EMRTEXT_OFFSET + 4;
  const EMRTEXT_CHARS_OFFSET: usize = EMRTEXT_OFFSET + 8;
  const EMRTEXT_STRING_OFFSET: usize = EMRTEXT_OFFSET + 12;
  const EMRTEXT_OPTIONS_OFFSET: usize = EMRTEXT_OFFSET + 16;
  const EMRTEXT_DX_OFFSET: usize = EMRTEXT_OFFSET + 36;
  let minimum_size = EMRTEXT_OFFSET + 40;
  if record_size < minimum_size {
    return None;
  }
  let characters = read_u32(data, record_offset + EMRTEXT_CHARS_OFFSET).ok()? as usize;
  let string_offset = read_u32(data, record_offset + EMRTEXT_STRING_OFFSET).ok()? as usize;
  if characters == 0 || string_offset >= record_size {
    return None;
  }
  Some(ExtTextRecord {
    graphics_mode: read_u32(data, record_offset + GRAPHICS_MODE_OFFSET).ok()?,
    x_scale: read_f32(data, record_offset + X_SCALE_OFFSET).ok()?,
    y_scale: read_f32(data, record_offset + Y_SCALE_OFFSET).ok()?,
    x: read_i32(data, record_offset + EMRTEXT_REFERENCE_X_OFFSET).ok()?,
    y: read_i32(data, record_offset + EMRTEXT_REFERENCE_Y_OFFSET).ok()?,
    characters,
    string_offset,
    options: read_u32(data, record_offset + EMRTEXT_OPTIONS_OFFSET).ok()?,
    dx_offset: match read_u32(data, record_offset + EMRTEXT_DX_OFFSET).ok()? as usize {
      0 => None,
      offset if offset < record_size => Some(offset),
      _ => None,
    },
  })
}

fn ext_text_advances(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
  text: ExtTextRecord,
) -> Option<Vec<i32>> {
  const ETO_PDY: u32 = 0x0000_2000;
  let dx_offset = text.dx_offset?;
  let stride = if text.options & ETO_PDY != 0 { 2 } else { 1 };
  let value_count = text.characters.checked_mul(stride)?;
  let byte_count = value_count.checked_mul(4)?;
  let start = record_offset.checked_add(dx_offset)?;
  let end = start.checked_add(byte_count)?;
  if end > record_offset.checked_add(record_size)? || end > data.len() {
    return None;
  }
  (0..text.characters)
    .map(|index| read_i32(data, start + index * stride * 4).ok())
    .collect()
}

fn ext_text_displacement(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
  text: ExtTextRecord,
) -> Option<EmfPoint> {
  const ETO_PDY: u32 = 0x0000_2000;
  let dx_offset = text.dx_offset?;
  let has_vertical_advances = text.options & ETO_PDY != 0;
  let stride = if has_vertical_advances { 2 } else { 1 };
  let value_count = text.characters.checked_mul(stride)?;
  let byte_count = value_count.checked_mul(4)?;
  let start = record_offset.checked_add(dx_offset)?;
  let end = start.checked_add(byte_count)?;
  if end > record_offset.checked_add(record_size)? || end > data.len() {
    return None;
  }
  let mut displacement = EmfPoint { x: 0, y: 0 };
  for index in 0..text.characters {
    let offset = start + index * stride * 4;
    displacement.x = displacement.x.saturating_add(read_i32(data, offset).ok()?);
    if has_vertical_advances {
      displacement.y = displacement
        .y
        .saturating_add(read_i32(data, offset + 4).ok()?);
    }
  }
  Some(displacement)
}

fn cumulative_mapped_advances(
  logical_advances: &[i32],
  mut map_cumulative: impl FnMut(i64) -> f32,
) -> Vec<f32> {
  let mut logical_cumulative = 0i64;
  let mut mapped_previous = 0.0f32;
  logical_advances
    .iter()
    .map(|advance| {
      logical_cumulative = logical_cumulative.saturating_add(i64::from(*advance));
      // ExtTextOut Dx values define consecutive logical advances, but their
      // device positions are obtained by mapping cumulative distances. Mapping
      // each small delta independently accumulates fractional pixel error.
      let mapped_cumulative = map_cumulative(logical_cumulative);
      let mapped_advance = mapped_cumulative - mapped_previous;
      mapped_previous = mapped_cumulative;
      mapped_advance
    })
    .collect()
}

fn read_logfont_object(
  data: &[u8],
  record_offset: usize,
  record_size: usize,
) -> Option<(u32, EmfFont)> {
  // EMR_EXTCREATEFONTINDIRECTW reads an object index followed by LOGFONTW.
  const OBJECT_ID_OFFSET: usize = 8;
  const LOGFONT_OFFSET: usize = 12;
  const LOGFONT_HEIGHT_OFFSET: usize = LOGFONT_OFFSET;
  const LOGFONT_WEIGHT_OFFSET: usize = LOGFONT_OFFSET + 16;
  const LOGFONT_ITALIC_OFFSET: usize = LOGFONT_OFFSET + 20;
  const LOGFONT_CHAR_SET_OFFSET: usize = LOGFONT_OFFSET + 23;
  const LOGFONT_QUALITY_OFFSET: usize = LOGFONT_OFFSET + 26;
  const LOGFONT_FACE_NAME_OFFSET: usize = LOGFONT_OFFSET + 28;
  let face_end = LOGFONT_FACE_NAME_OFFSET.checked_add(LOGFONT_FACE_NAME_CHARS * 2)?;
  if record_size < face_end {
    return None;
  }
  let object_id = read_u32(data, record_offset + OBJECT_ID_OFFSET).ok()?;
  let height = read_i32(data, record_offset + LOGFONT_HEIGHT_OFFSET).ok()?;
  let weight = read_i32(data, record_offset + LOGFONT_WEIGHT_OFFSET)
    .ok()?
    .clamp(0, 1000) as u16;
  let italic = *data.get(record_offset + LOGFONT_ITALIC_OFFSET)? != 0;
  let char_set = *data.get(record_offset + LOGFONT_CHAR_SET_OFFSET)?;
  let quality = *data.get(record_offset + LOGFONT_QUALITY_OFFSET)?;
  let face_bytes = data.get(
    record_offset + LOGFONT_FACE_NAME_OFFSET
      ..record_offset + LOGFONT_FACE_NAME_OFFSET + LOGFONT_FACE_NAME_CHARS * 2,
  )?;
  let family = String::from_utf16_lossy(
    &face_bytes
      .chunks_exact(2)
      .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
      .take_while(|unit| *unit != 0)
      .collect::<Vec<_>>(),
  );
  Some((
    object_id,
    EmfFont {
      height,
      family: (!family.is_empty()).then_some(family),
      char_set,
      weight,
      italic,
      quality,
    },
  ))
}

fn read_i16(data: &[u8], offset: usize) -> Result<i16, String> {
  let bytes = data
    .get(offset..offset + 2)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_f32(data: &[u8], offset: usize) -> Result<f32, String> {
  let bytes = data
    .get(offset..offset + 4)
    .ok_or_else(|| format!("read past end of buffer at offset {offset}"))?;
  Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn apply_binary_raster_operation(
  pen: EmfColor,
  destination: EmfColor,
  operation: WmfBinaryRasterOperation,
) -> EmfColor {
  let apply = |pen: u8, destination: u8| match operation {
    WmfBinaryRasterOperation::Black => 0,
    WmfBinaryRasterOperation::NotMergePen => !(destination | pen),
    WmfBinaryRasterOperation::MaskNotPen => destination & !pen,
    WmfBinaryRasterOperation::NotCopyPen => !pen,
    WmfBinaryRasterOperation::MaskPenNot => pen & !destination,
    WmfBinaryRasterOperation::Not => !destination,
    WmfBinaryRasterOperation::XorPen => destination ^ pen,
    WmfBinaryRasterOperation::NotMaskPen => !(destination & pen),
    WmfBinaryRasterOperation::MaskPen => destination & pen,
    WmfBinaryRasterOperation::NotXorPen => !(destination ^ pen),
    WmfBinaryRasterOperation::Nop => destination,
    WmfBinaryRasterOperation::MergeNotPen => destination | !pen,
    WmfBinaryRasterOperation::CopyPen => pen,
    WmfBinaryRasterOperation::MergePenNot => pen | !destination,
    WmfBinaryRasterOperation::MergePen => destination | pen,
    WmfBinaryRasterOperation::White => u8::MAX,
  };
  EmfColor {
    r: apply(pen.r, destination.r),
    g: apply(pen.g, destination.g),
    b: apply(pen.b, destination.b),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::emf::{EmrAlphaBlend, EmrBitBlt, EmrBlendFunction, EmrMapMode, EmrStretchBlt};
  use crate::emfplus::{
    EmfPlusGraphicsVersion, EmfPlusGraphicsVersionValue, EmfPlusHeaderData, EmfPlusRegionObject,
    EmfPlusSetPageTransformData, EmfPlusStream,
  };
  use crate::wmf::{
    WmfColorRecord, WmfDibCreatePatternBrushRecord, WmfDibStretchBltRecord, WmfDibTarget,
    WmfExtTextOutRecord, WmfLogBrushObject, WmfMetafileType, WmfMetafileVersion,
    WmfObjectIndexRecord, WmfPatBltRecord, WmfPointRecord, WmfRectObject, WmfSetPixelRecord,
    WmfU16Record,
  };
  use crate::{
    BitmapSourceBounds, ColorRef, DibColorUsage, EMR_EOF, EMR_HEADER, EmfMetafile, EmfRecord,
    EmfRecordData, EmrBitmapBuffer, EmrStretchDiBits, PointL, RectL, SdkEnumValue, SizeL,
    WmfHeader, WmfMetafile, WmfRecord, WmfRecordData, XForm,
  };

  #[test]
  fn gdi_font_metrics_are_realized_on_the_integer_device_grid() {
    assert_eq!(gdi_realized_font_metric(23.740_234), 24.0);
    assert_eq!(gdi_realized_font_metric(19.009_277), 19.0);
    assert_eq!(gdi_realized_font_metric(-5.6), -6.0);
    assert_eq!(gdi_realized_font_metric(-5.4), -5.0);
  }

  fn header_record(right: i32, bottom: i32) -> EmfRecord {
    let mut data = vec![0; 100];
    data[8..12].copy_from_slice(&right.to_le_bytes());
    data[12..16].copy_from_slice(&bottom.to_le_bytes());
    data[32..36].copy_from_slice(&crate::emf::EMF_SIGNATURE.to_le_bytes());
    EmfRecord::new(EMR_HEADER, data)
  }

  fn eof_record() -> EmfRecord {
    EmfRecord::new(EMR_EOF, vec![0; 12])
  }

  fn set_text_align_record(alignment: WmfTextAlignmentModeFlags) -> EmfRecord {
    EmfRecord::new(
      super::EMR_SET_TEXT_ALIGN,
      u32::from(alignment.bits()).to_le_bytes().to_vec(),
    )
  }

  fn set_map_mode_record(map_mode: EmrMapMode) -> EmfRecord {
    EmfRecord::new(
      super::EMR_SET_MAP_MODE,
      map_mode.raw().to_le_bytes().to_vec(),
    )
  }

  fn extent_record(record_type: u32, x: i32, y: i32) -> EmfRecord {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    EmfRecord::new(record_type, data)
  }

  fn move_to_ex_record(x: i32, y: i32) -> EmfRecord {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    EmfRecord::new(super::EMR_MOVE_TO_EX, data)
  }

  fn ext_text_out_w_record(x: i32, y: i32, text: &str) -> EmfRecord {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut data = vec![0; 68];
    data[16..20].copy_from_slice(&1u32.to_le_bytes());
    data[20..24].copy_from_slice(&1.0f32.to_le_bytes());
    data[24..28].copy_from_slice(&1.0f32.to_le_bytes());
    data[28..32].copy_from_slice(&x.to_le_bytes());
    data[32..36].copy_from_slice(&y.to_le_bytes());
    data[36..40].copy_from_slice(&(units.len() as u32).to_le_bytes());
    data[40..44].copy_from_slice(&76u32.to_le_bytes());
    for unit in units {
      data.extend_from_slice(&unit.to_le_bytes());
    }
    while !data.len().is_multiple_of(4) {
      data.push(0);
    }
    let dx_offset = (data.len() + 8) as u32;
    data[64..68].copy_from_slice(&dx_offset.to_le_bytes());
    for _ in text.encode_utf16() {
      data.extend_from_slice(&8i32.to_le_bytes());
    }
    EmfRecord::new(super::EMR_EXT_TEXTOUT_W, data)
  }

  fn bitmap_info(width: i32, height: i32, bit_count: u16, compression: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&bit_count.to_le_bytes());
    bytes.extend_from_slice(&compression.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
  }

  fn two_pixel_monochrome_dib(bits: u8) -> Vec<u8> {
    let mut dib = bitmap_info(2, 1, 1, 0);
    dib.extend_from_slice(&[
      0, 0, 0, 0, // palette entry zero: black
      255, 255, 255, 0, // palette entry one: white
    ]);
    dib.extend_from_slice(&[bits, 0, 0, 0]);
    dib
  }

  fn two_pixel_color_dib() -> Vec<u8> {
    let mut dib = bitmap_info(2, 1, 24, 0);
    dib.extend_from_slice(&[
      0, 0, 255, // red in BGR order
      0, 255, 0, // green in BGR order
      0, 0, // DWORD scan-line padding
    ]);
    dib
  }

  fn two_pixel_non_binary_mask_dib() -> Vec<u8> {
    let mut dib = bitmap_info(2, 1, 24, 0);
    dib.extend_from_slice(&[
      128, 128, 128, // gray is not a valid binary transparency mask
      255, 255, 255, // white
      0, 0,
    ]);
    dib
  }

  fn masked_bitmap_wmf(mask: Vec<u8>, source: Vec<u8>) -> Vec<u8> {
    let dib_record = |operation, target| {
      WmfRecordData::DibStretchBlt(WmfDibStretchBltRecord {
        raster_operation: operation,
        src_height: 1,
        src_width: 2,
        y_src: 0,
        x_src: 0,
        dest_height: 1,
        dest_width: 2,
        y_dest: 0,
        x_dest: 1,
        target: WmfDibTarget::Source(target),
      })
      .to_record()
      .unwrap()
    };
    WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 0,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records: vec![
        WmfRecordData::SetWindowExt(WmfPointRecord { x: 4, y: 2 })
          .to_record()
          .unwrap(),
        dib_record(WmfTernaryRasterOperationCode::SRCAND.canonical_raw(), mask),
        dib_record(
          WmfTernaryRasterOperationCode::SRCPAINT.canonical_raw(),
          source,
        ),
        WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
          .to_record()
          .unwrap(),
      ],
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap()
  }

  fn copy_bitmap_wmf() -> Vec<u8> {
    WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 0,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records: vec![
        WmfRecordData::SetWindowExt(WmfPointRecord { x: 4, y: 2 })
          .to_record()
          .unwrap(),
        WmfRecordData::DibStretchBlt(WmfDibStretchBltRecord {
          raster_operation: WmfTernaryRasterOperationCode::SRCCOPY.canonical_raw(),
          src_height: 1,
          src_width: 2,
          y_src: 0,
          x_src: 0,
          dest_height: 1,
          dest_width: 2,
          y_dest: 0,
          x_dest: 1,
          target: WmfDibTarget::Source(two_pixel_color_dib()),
        })
        .to_record()
        .unwrap(),
        WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
          .to_record()
          .unwrap(),
      ],
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap()
  }

  #[test]
  fn emf_natural_canvas_uses_frame_and_reference_device() {
    let mut data = vec![0; EMF_HEADER_SIZE];
    data[EMF_BOUNDS_LEFT_OFFSET..EMF_BOUNDS_LEFT_OFFSET + 4].copy_from_slice(&16i32.to_le_bytes());
    data[EMF_BOUNDS_TOP_OFFSET..EMF_BOUNDS_TOP_OFFSET + 4].copy_from_slice(&1i32.to_le_bytes());
    data[EMF_BOUNDS_RIGHT_OFFSET..EMF_BOUNDS_RIGHT_OFFSET + 4]
      .copy_from_slice(&84i32.to_le_bytes());
    data[EMF_BOUNDS_BOTTOM_OFFSET..EMF_BOUNDS_BOTTOM_OFFSET + 4]
      .copy_from_slice(&46i32.to_le_bytes());
    data[EMF_FRAME_RIGHT_OFFSET..EMF_FRAME_RIGHT_OFFSET + 4]
      .copy_from_slice(&2580i32.to_le_bytes());
    data[EMF_FRAME_BOTTOM_OFFSET..EMF_FRAME_BOTTOM_OFFSET + 4]
      .copy_from_slice(&1597i32.to_le_bytes());
    data[EMF_DEVICE_WIDTH_OFFSET..EMF_DEVICE_WIDTH_OFFSET + 4]
      .copy_from_slice(&1920i32.to_le_bytes());
    data[EMF_DEVICE_HEIGHT_OFFSET..EMF_DEVICE_HEIGHT_OFFSET + 4]
      .copy_from_slice(&1080i32.to_le_bytes());
    data[EMF_MILLIMETERS_WIDTH_OFFSET..EMF_MILLIMETERS_WIDTH_OFFSET + 4]
      .copy_from_slice(&480i32.to_le_bytes());
    data[EMF_MILLIMETERS_HEIGHT_OFFSET..EMF_MILLIMETERS_HEIGHT_OFFSET + 4]
      .copy_from_slice(&260i32.to_le_bytes());

    assert_eq!(emf_natural_canvas_size(&data).unwrap(), (103, 66));
    let physical = emf_physical_size(&data).unwrap();
    assert!((physical.width_pt - 73.133_86).abs() < 0.000_1);
    assert!((physical.height_pt - 45.269_29).abs() < 0.000_1);
    assert_eq!(physical.natural_width_px, 103);
    assert_eq!(physical.natural_height_px, 66);
  }

  #[test]
  fn emf_replay_starts_after_a_variable_header_description() {
    let mut emf = metafile_with_header_bounds(1, 1, vec![set_pixel_record(0, 0, 0x0000_00ff)]);
    let description = [b'A', 0, b'p', 0, b'p', 0, 0, 0];
    emf.splice(EMF_HEADER_SIZE..EMF_HEADER_SIZE, description);
    let header_size = EMF_HEADER_SIZE + description.len();
    let metafile_size = emf.len();
    emf[4..8].copy_from_slice(&(header_size as u32).to_le_bytes());
    emf[48..52].copy_from_slice(&(metafile_size as u32).to_le_bytes());
    emf[60..64].copy_from_slice(&4u32.to_le_bytes());
    emf[64..68].copy_from_slice(&(EMF_HEADER_SIZE as u32).to_le_bytes());

    assert_eq!(emf_header_record_size(&emf), Some(header_size));
    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn emf_playback_maps_nonzero_frame_origin_to_the_output_surface() {
    let mut emf = metafile_with_header_bounds(59, 39, vec![set_pixel_record(50, 30, 0x0000_00ff)]);
    emf[EMF_BOUNDS_LEFT_OFFSET..EMF_BOUNDS_LEFT_OFFSET + 4].copy_from_slice(&50i32.to_le_bytes());
    emf[EMF_BOUNDS_TOP_OFFSET..EMF_BOUNDS_TOP_OFFSET + 4].copy_from_slice(&30i32.to_le_bytes());
    emf[EMF_FRAME_LEFT_OFFSET..EMF_FRAME_LEFT_OFFSET + 4].copy_from_slice(&1270i32.to_le_bytes());
    emf[EMF_FRAME_TOP_OFFSET..EMF_FRAME_TOP_OFFSET + 4].copy_from_slice(&762i32.to_le_bytes());
    emf[EMF_FRAME_RIGHT_OFFSET..EMF_FRAME_RIGHT_OFFSET + 4].copy_from_slice(&1524i32.to_le_bytes());
    emf[EMF_FRAME_BOTTOM_OFFSET..EMF_FRAME_BOTTOM_OFFSET + 4]
      .copy_from_slice(&1016i32.to_le_bytes());
    emf[EMF_DEVICE_WIDTH_OFFSET..EMF_DEVICE_WIDTH_OFFSET + 4]
      .copy_from_slice(&1000i32.to_le_bytes());
    emf[EMF_DEVICE_HEIGHT_OFFSET..EMF_DEVICE_HEIGHT_OFFSET + 4]
      .copy_from_slice(&1000i32.to_le_bytes());
    emf[EMF_MILLIMETERS_WIDTH_OFFSET..EMF_MILLIMETERS_WIDTH_OFFSET + 4]
      .copy_from_slice(&254i32.to_le_bytes());
    emf[EMF_MILLIMETERS_HEIGHT_OFFSET..EMF_MILLIMETERS_HEIGHT_OFFSET + 4]
      .copy_from_slice(&254i32.to_le_bytes());

    let decoded =
      decode_vector_emf_as_png(&emf, RenderOptions::default(), GdiTextSurface::Color).unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();

    assert_eq!(image.dimensions(), (10, 10));
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn emf_plus_set_page_transform_replaces_page_state_and_preserves_world_state() {
    let data = metafile_with_header_bounds(9, 9, Vec::new());
    let mut state = EmfVectorState::new_with_options(&data, RenderOptions::default()).unwrap();
    process_emf_plus_record(
      EmfPlusRecordData::Header(EmfPlusHeaderData {
        graphics_version: EmfPlusGraphicsVersion::from_graphics_version(
          EmfPlusGraphicsVersionValue::Version1_1,
        ),
        emf_plus_flags: 1,
        logical_dpi_x: 96,
        logical_dpi_y: 120,
      }),
      EmfPlusRecordFlags::empty(),
      &mut state,
    )
    .unwrap();
    for page_scale in [2.0, 0.5] {
      process_emf_plus_record(
        EmfPlusRecordData::SetPageTransform(EmfPlusSetPageTransformData { page_scale }),
        EmfPlusRecordFlags::from_bits_retain(EmfPlusUnitType::Pixel.raw() as u16),
        &mut state,
      )
      .unwrap();
    }

    let (x, y) = state.map_point(EmfPoint { x: 4, y: 6 });
    assert!((x - 2.0).abs() < f32::EPSILON);
    assert!((y - 3.0).abs() < f32::EPSILON);
    assert_eq!(state.emf_plus_page_unit, EmfPlusUnitType::Pixel);
    assert!((state.emf_plus_page_scale - 0.5).abs() < f32::EPSILON);
    assert!((state.world_transform.m11 - 1.0).abs() < f32::EPSILON);
    assert!((state.world_transform.m22 - 1.0).abs() < f32::EPSILON);

    assert!(
      (emf_plus_units_to_device_scale(EmfPlusUnitType::Point, 1.5, 96.0, true) - 2.0).abs()
        < f32::EPSILON
    );
  }

  #[test]
  fn vdmx_selects_exact_ansi_one_to_one_device_metrics() {
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // version
    table.extend_from_slice(&1u16.to_be_bytes()); // groups
    table.extend_from_slice(&1u16.to_be_bytes()); // ratios
    table.extend_from_slice(&[1, 1, 1, 1]); // ANSI subset, 1:1 device
    table.extend_from_slice(&12u16.to_be_bytes()); // group offset
    table.extend_from_slice(&1u16.to_be_bytes()); // records
    table.extend_from_slice(&[12, 14]); // supported ppem range
    table.extend_from_slice(&13u16.to_be_bytes());
    table.extend_from_slice(&14i16.to_be_bytes());
    table.extend_from_slice(&(-3i16).to_be_bytes());

    assert_eq!(
      vdmx_vertical_device_metrics(&table, 13, crate::wmf::WmfCharacterSet::Ansi.raw(),),
      Some(GdiVerticalDeviceMetrics {
        ascent: 14,
        descent: 3,
      })
    );
    // VDMX records are sparse: an in-range ppem without an exact record must
    // retain the linearly scaled font metrics.
    assert_eq!(
      vdmx_vertical_device_metrics(&table, 12, crate::wmf::WmfCharacterSet::Ansi.raw(),),
      None
    );
  }

  #[test]
  fn vdmx_rejects_charset_aspect_and_truncated_mismatches() {
    let make_table = |ratio: [u8; 4]| {
      let mut table = Vec::new();
      table.extend_from_slice(&0u16.to_be_bytes());
      table.extend_from_slice(&1u16.to_be_bytes());
      table.extend_from_slice(&1u16.to_be_bytes());
      table.extend_from_slice(&ratio);
      table.extend_from_slice(&12u16.to_be_bytes());
      table.extend_from_slice(&1u16.to_be_bytes());
      table.extend_from_slice(&[13, 13]);
      table.extend_from_slice(&13u16.to_be_bytes());
      table.extend_from_slice(&14i16.to_be_bytes());
      table.extend_from_slice(&(-3i16).to_be_bytes());
      table
    };
    let ansi = crate::wmf::WmfCharacterSet::Ansi.raw();
    let symbol = crate::wmf::WmfCharacterSet::Symbol.raw();
    let ansi_table = make_table([1, 1, 1, 1]);
    let wrong_aspect = make_table([1, 4, 3, 3]);

    assert_eq!(vdmx_vertical_device_metrics(&ansi_table, 13, symbol), None);
    assert_eq!(vdmx_vertical_device_metrics(&wrong_aspect, 13, ansi), None);
    assert_eq!(
      vdmx_vertical_device_metrics(&ansi_table[..ansi_table.len() - 1], 13, ansi),
      None
    );
  }

  #[test]
  fn emf_logfont_preserves_visible_text_face_properties() {
    let mut record = vec![0; 104];
    record[8..12].copy_from_slice(&7u32.to_le_bytes());
    record[12..16].copy_from_slice(&(-11i32).to_le_bytes());
    record[28..32].copy_from_slice(&700i32.to_le_bytes());
    record[32] = 1;
    record[35] = crate::wmf::WmfCharacterSet::Greek.raw();
    record[38] = crate::wmf::WmfFontQuality::ClearType.raw();
    for (index, unit) in "Segoe UI".encode_utf16().enumerate() {
      let offset = 40 + index * 2;
      record[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
    }

    let (object_id, font) = read_logfont_object(&record, 0, record.len()).unwrap();
    assert_eq!(object_id, 7);
    assert_eq!(font.height, -11);
    assert_eq!(font.family.as_deref(), Some("Segoe UI"));
    assert_eq!(font.weight, 700);
    assert!(font.italic);
    assert_eq!(font.char_set, crate::wmf::WmfCharacterSet::Greek.raw());
    assert_eq!(font.quality, crate::wmf::WmfFontQuality::ClearType.raw());
  }

  #[test]
  fn emf_ext_text_out_preserves_graphics_scales_and_cumulative_dx_mapping() {
    let mut record = vec![0; 96];
    record[24..28].copy_from_slice(&1u32.to_le_bytes());
    record[28..32].copy_from_slice(&25.0f32.to_le_bytes());
    record[32..36].copy_from_slice(&24.074_074f32.to_le_bytes());
    record[36..40].copy_from_slice(&16i32.to_le_bytes());
    record[40..44].copy_from_slice(&34i32.to_le_bytes());
    record[44..48].copy_from_slice(&2u32.to_le_bytes());
    record[48..52].copy_from_slice(&80u32.to_le_bytes());
    record[72..76].copy_from_slice(&84u32.to_le_bytes());
    record[84..88].copy_from_slice(&6i32.to_le_bytes());
    record[88..92].copy_from_slice(&3i32.to_le_bytes());

    let text = ext_text_record(&record, 0, record.len()).unwrap();
    assert_eq!(text.graphics_mode, 1);
    assert_eq!(text.x_scale, 25.0);
    assert_eq!(text.y_scale, 24.074_074);
    assert_eq!((text.x, text.y), (16, 34));
    assert_eq!(
      ext_text_advances(&record, 0, record.len(), text).unwrap(),
      [6, 3]
    );

    let mapped = cumulative_mapped_advances(&[6, 3, 4], |logical| {
      (logical as f32 * 214.0 / 103.0).round()
    });
    assert_eq!(mapped, [12.0, 7.0, 8.0]);
  }

  #[test]
  fn emf_semantic_text_honors_text_alignment_reference_point() {
    let top = metafile_with(ext_text_out_w_record(0, 0, "A"));
    let baseline = metafile_with_records(vec![
      set_text_align_record(WmfTextAlignmentModeFlags::BASELINE),
      ext_text_out_w_record(0, 0, "A"),
    ]);

    let top_run = extract_metafile_text_runs(&top, Some("image/x-emf"))
      .pop()
      .expect("default TA_TOP text");
    let baseline_run = extract_metafile_text_runs(&baseline, Some("image/x-emf"))
      .pop()
      .expect("TA_BASELINE text");

    assert!(top_run.y > baseline_run.y);
    assert_eq!(baseline_run.y, 0.0);
  }

  #[test]
  fn emf_semantic_text_honors_horizontal_text_alignment() {
    let left = metafile_with(ext_text_out_w_record(20, 0, "AB"));
    let center = metafile_with_records(vec![
      set_text_align_record(WmfTextAlignmentModeFlags::CENTER),
      ext_text_out_w_record(20, 0, "AB"),
    ]);
    let right = metafile_with_records(vec![
      set_text_align_record(WmfTextAlignmentModeFlags::RIGHT),
      ext_text_out_w_record(20, 0, "AB"),
    ]);

    let left_x = extract_metafile_text_runs(&left, Some("image/x-emf"))[0].x;
    let center_x = extract_metafile_text_runs(&center, Some("image/x-emf"))[0].x;
    let right_x = extract_metafile_text_runs(&right, Some("image/x-emf"))[0].x;
    let left_width = extract_metafile_text_runs(&left, Some("image/x-emf"))[0]
      .width
      .expect("Dx width");

    assert!(right_x < center_x);
    assert!(center_x < left_x);
    assert_eq!(left_width, 8.0);
  }

  #[test]
  fn emf_update_cp_uses_move_to_and_dx_for_consecutive_text_origins() {
    let emf = metafile_with_records(vec![
      set_text_align_record(
        WmfTextAlignmentModeFlags::UPDATE_CP | WmfTextAlignmentModeFlags::BASELINE,
      ),
      move_to_ex_record(10, 20),
      ext_text_out_w_record(99, 88, "AB"),
      ext_text_out_w_record(99, 88, "C"),
    ]);
    let (width, height) = emf_natural_canvas_size(&emf).unwrap();
    let runs = extract_metafile_text_runs(&emf, Some("image/x-emf"));

    assert_eq!(runs.len(), 2);
    assert!((runs[0].x * width as f32 - 10.0).abs() < 0.000_1);
    assert!((runs[0].y * height as f32 - 20.0).abs() < 0.000_1);
    assert!((runs[1].x * width as f32 - 26.0).abs() < 0.000_1);
    assert!((runs[1].y * height as f32 - 20.0).abs() < 0.000_1);
  }

  #[test]
  fn emf_text_mode_ignores_extents_while_anisotropic_applies_them() {
    let mapped_metafile = |map_mode: Option<EmrMapMode>, record| {
      let mut records = Vec::new();
      if let Some(map_mode) = map_mode {
        records.push(set_map_mode_record(map_mode));
      }
      records.push(extent_record(super::EMR_SET_WINDOW_EXT_EX, 4, 4));
      records.push(extent_record(super::EMR_SET_VIEWPORT_EXT_EX, 20, 20));
      records.push(record);
      metafile_with_header_bounds(19, 19, records)
    };

    let text_pixels = mapped_metafile(None, set_pixel_record(2, 3, 0x0000_00ff));
    let anisotropic_pixels = mapped_metafile(
      Some(EmrMapMode::Anisotropic),
      set_pixel_record(2, 3, 0x0000_00ff),
    );
    let decode = |data: &[u8]| {
      let decoded =
        decode_vector_emf_as_png(data, RenderOptions::default(), GdiTextSurface::Color).unwrap();
      image::load_from_memory(&decoded.data).unwrap().to_rgb8()
    };
    let text_image = decode(&text_pixels);
    let anisotropic_image = decode(&anisotropic_pixels);

    assert_eq!(text_image.dimensions(), (20, 20));
    assert_eq!(text_image.get_pixel(2, 3).0, [255, 0, 0]);
    assert_eq!(text_image.get_pixel(10, 15).0, [255, 255, 255]);
    assert_eq!(anisotropic_image.get_pixel(2, 3).0, [255, 255, 255]);
    assert_eq!(anisotropic_image.get_pixel(10, 15).0, [255, 0, 0]);

    let text_runs = mapped_metafile(None, ext_text_out_w_record(2, 3, "A"));
    let anisotropic_runs = mapped_metafile(
      Some(EmrMapMode::Anisotropic),
      ext_text_out_w_record(2, 3, "A"),
    );
    let text_x = extract_metafile_text_runs(&text_runs, Some("image/x-emf"))[0].x;
    let anisotropic_x = extract_metafile_text_runs(&anisotropic_runs, Some("image/x-emf"))[0].x;

    assert!((text_x * 20.0 - 2.0).abs() < 0.000_1);
    assert!((anisotropic_x * 20.0 - 10.0).abs() < 0.000_1);
  }

  #[test]
  fn emf_text_marks_only_runs_after_destination_dependent_raster_operations() {
    let source_bits = vec![0; 16];
    let emf = metafile_with_records(vec![
      ext_text_out_w_record(0, 0, "before"),
      stretch_blt_record(
        bitmap_info(2, 2, 32, BI_RGB),
        source_bits.clone(),
        0x00CC_0020,
      ),
      ext_text_out_w_record(0, 0, "after copy"),
      stretch_blt_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0088_00C6),
      ext_text_out_w_record(0, 0, "after destination readback"),
    ]);

    let runs = extract_metafile_text_runs(&emf, Some("image/x-emf"));
    assert_eq!(runs.len(), 3);
    assert!(!runs[0].requires_raster_backdrop);
    assert!(!runs[1].requires_raster_backdrop);
    assert!(runs[2].requires_raster_backdrop);
  }

  #[test]
  fn line_clip_limits_huge_off_canvas_segments() {
    let clipped = clip_line_to_rect(
      (-1_000_000_000.0, 5.0),
      (1_000_000_000.0, 5.0),
      (0.0, 0.0, 9.0, 9.0),
    )
    .expect("horizontal line crosses the canvas");
    assert!((clipped.0.0 - 0.0).abs() < 0.001);
    assert!((clipped.0.1 - 5.0).abs() < 0.001);
    assert!((clipped.1.0 - 9.0).abs() < 0.001);
    assert!((clipped.1.1 - 5.0).abs() < 0.001);

    assert_eq!(
      clip_line_to_rect(
        (-1_000_000_000.0, -5.0),
        (1_000_000_000.0, -5.0),
        (0.0, 0.0, 9.0, 9.0),
      ),
      None
    );
  }

  #[test]
  fn render_target_defines_the_playback_viewport_in_both_directions() {
    assert_eq!(
      RenderOptions {
        target_width_px: Some(200),
        target_height_px: Some(100),
        max_pixels: None,
        transparent_background: false,
        background_color: None,
        monochrome_dib_palette_override: None,
        filter_high_frequency_pattern_brushes: false,
        suppress_text: false,
        suppress_solid_pattern_rects: false,
        suppress_bitmap_layers: false,
        wmf_external_header: None,
      }
      .resolved_canvas_size(400, 300),
      (200, 100)
    );
    assert_eq!(
      RenderOptions {
        target_width_px: Some(400),
        target_height_px: Some(300),
        max_pixels: None,
        transparent_background: false,
        background_color: None,
        monochrome_dib_palette_override: None,
        filter_high_frequency_pattern_brushes: false,
        suppress_text: false,
        suppress_solid_pattern_rects: false,
        suppress_bitmap_layers: false,
        wmf_external_header: None,
      }
      .resolved_canvas_size(76, 76),
      (400, 300)
    );

    let mut state = EmfVectorState::new_with_options(
      &metafile_with_records(Vec::new()),
      RenderOptions {
        target_width_px: Some(4),
        target_height_px: Some(3),
        ..RenderOptions::default()
      },
    )
    .expect("minimal EMF playback state");
    // EMR_SET{WINDOW,VIEWPORT}EXT records describe the metafile's logical to
    // natural-device mapping. They must not discard the player's outer
    // target-viewport transform.
    let (natural_width, natural_height) =
      emf_natural_canvas_size(&metafile_with_records(Vec::new())).unwrap();
    state.window_ext_x = natural_width as i32;
    state.window_ext_y = natural_height as i32;
    state.viewport_ext_x = natural_width as i32;
    state.viewport_ext_y = natural_height as i32;
    assert_eq!(
      state.map_point(EmfPoint {
        x: natural_width as i32,
        y: natural_height as i32,
      }),
      (4.0, 3.0)
    );
  }

  #[test]
  fn wmf_ext_text_out_opaque_fills_background_and_restores_temporary_clip() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 8, y: 8 })
        .to_record()
        .unwrap(),
      WmfRecordData::SetBkColor(WmfColorRecord {
        color: crate::ColorRef {
          red: 0,
          green: 192,
          blue: 0,
          reserved: 0,
        },
      })
      .to_record()
      .unwrap(),
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 1,
        x: 1,
        string_length: 0,
        options: WmfExtTextOutOptions::OPAQUE | WmfExtTextOutOptions::CLIPPED,
        rectangle: Some(WmfRectObject {
          left: 1,
          top: 1,
          right: 7,
          bottom: 7,
        }),
        string: Vec::new(),
        string_padding: Vec::new(),
        dx: Vec::new(),
        trailing_data: Vec::new(),
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SetPixel(WmfSetPixelRecord {
        color: crate::ColorRef {
          red: 255,
          green: 0,
          blue: 0,
          reserved: 0,
        },
        y: 0,
        x: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let metafile = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 0,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    };

    let decoded = decode_metafile_as_raster(&metafile.to_bytes().unwrap(), Some("image/x-wmf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();

    assert_eq!(image.get_pixel(4, 4).0, [0, 192, 0]);
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(7, 7).0, [255, 255, 255]);
  }

  #[test]
  fn wmf_text_can_be_lifted_out_of_raster_without_suppressing_graphics() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 32, y: 20 })
        .to_record()
        .unwrap(),
      WmfRecordData::SetPixel(WmfSetPixelRecord {
        color: crate::ColorRef {
          red: 255,
          green: 0,
          blue: 0,
          reserved: 0,
        },
        y: 0,
        x: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 1,
        x: 4,
        string_length: 1,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"A".to_vec(),
        string_padding: vec![0],
        dx: vec![8],
        trailing_data: Vec::new(),
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let bytes = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 0,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap();

    let decode = |suppress_text| {
      let decoded = decode_metafile_as_raster_with_options(
        &bytes,
        Some("image/x-wmf"),
        RenderOptions {
          suppress_text,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap();
      image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
        .unwrap()
        .to_rgb8()
    };
    let painted = decode(false);
    let lifted = decode(true);

    assert_eq!(painted.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(lifted.get_pixel(0, 0).0, [255, 0, 0]);
    assert!(painted.pixels().any(|pixel| pixel.0 == [0, 0, 0]));
    assert!(
      lifted
        .enumerate_pixels()
        .all(|(x, y, pixel)| (x == 0 && y == 0) || pixel.0 == [255, 255, 255])
    );
  }

  #[test]
  fn wmf_solid_patcopy_rects_can_be_lifted_without_suppressing_other_graphics() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 8, y: 8 })
        .to_record()
        .unwrap(),
      WmfRecordData::CreateBrushIndirect(WmfLogBrushObject {
        brush_style: WmfBrushStyle::Solid.raw(),
        color_ref: ColorRef {
          red: 255,
          green: 0,
          blue: 0,
          reserved: 0,
        },
        brush_hatch: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SelectObject(WmfObjectIndexRecord { index: 0 })
        .to_record()
        .unwrap(),
      WmfRecordData::PatBlt(WmfPatBltRecord {
        raster_operation: WmfTernaryRasterOperationCode::PATCOPY.canonical_raw(),
        height: 2,
        width: 4,
        y_left: 3,
        x_left: 2,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SetPixel(WmfSetPixelRecord {
        color: ColorRef {
          red: 0,
          green: 0,
          blue: 255,
          reserved: 0,
        },
        y: 0,
        x: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let bytes = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 1,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap();

    assert_eq!(
      extract_metafile_solid_rects(&bytes, Some("image/x-wmf")),
      vec![MetafileSolidRect {
        x: 0.25,
        y: 0.375,
        width: 0.5,
        height: 0.25,
        color: [255, 0, 0],
      }]
    );

    let decode = |suppress_solid_pattern_rects| {
      let decoded = decode_metafile_as_raster_with_options(
        &bytes,
        Some("image/x-wmf"),
        RenderOptions {
          suppress_solid_pattern_rects,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap();
      image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
        .unwrap()
        .to_rgb8()
    };
    let painted = decode(false);
    let lifted = decode(true);

    assert_eq!(painted.get_pixel(2, 3).0, [255, 0, 0]);
    assert_eq!(lifted.get_pixel(2, 3).0, [255, 255, 255]);
    assert_eq!(painted.get_pixel(0, 0).0, [0, 0, 255]);
    assert_eq!(lifted.get_pixel(0, 0).0, [0, 0, 255]);
  }

  #[test]
  fn non_placeable_wmf_external_header_uses_the_reference_device_grid() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 8, y: 8 })
        .to_record()
        .unwrap(),
      WmfRecordData::CreateBrushIndirect(WmfLogBrushObject {
        brush_style: WmfBrushStyle::Solid.raw(),
        color_ref: ColorRef {
          red: 255,
          green: 0,
          blue: 0,
          reserved: 0,
        },
        brush_hatch: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SelectObject(WmfObjectIndexRecord { index: 0 })
        .to_record()
        .unwrap(),
      WmfRecordData::PatBlt(WmfPatBltRecord {
        raster_operation: WmfTernaryRasterOperationCode::PATCOPY.canonical_raw(),
        height: 2,
        width: 4,
        y_left: 3,
        x_left: 2,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let bytes = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 1,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap();
    let external_header = WmfExternalHeader {
      // 212 mm100 rounds to eight pixels at the sourced 96-DPI reference.
      width_hundredths_mm: 212,
      height_hundredths_mm: 212,
      reference_device_dpi_x: 96,
      reference_device_dpi_y: 96,
    };
    let external = extract_metafile_solid_rects_with_options(
      &bytes,
      Some("image/x-wmf"),
      RenderOptions {
        wmf_external_header: Some(external_header),
        ..RenderOptions::default()
      },
    );
    assert_eq!(external.len(), 1);
    assert!((external[0].x - 2.0 / 8.0).abs() < f32::EPSILON);
    assert!((external[0].y - 3.0 / 8.0).abs() < f32::EPSILON);
    assert!((external[0].width - 4.0 / 8.0).abs() < f32::EPSILON);
    assert!((external[0].height - 2.0 / 8.0).abs() < f32::EPSILON);

    let decoded = decode_metafile_as_raster_with_options(
      &bytes,
      Some("image/x-wmf"),
      RenderOptions {
        wmf_external_header: Some(external_header),
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();
    assert_eq!(image.dimensions(), (8, 8));

    let windows_probe_header = WmfExternalHeader {
      width_hundredths_mm: 5_080,
      height_hundredths_mm: 3_016,
      reference_device_dpi_x: 140,
      reference_device_dpi_y: 140,
    };
    assert_eq!(
      wmf_external_canvas_size(
        &WmfMetafileRef::from_bytes(&bytes).unwrap(),
        Some(windows_probe_header)
      ),
      Some((280, 166))
    );

    let ignored_zero_header = extract_metafile_solid_rects_with_options(
      &bytes,
      Some("image/x-wmf"),
      RenderOptions {
        wmf_external_header: Some(WmfExternalHeader {
          width_hundredths_mm: 0,
          height_hundredths_mm: 212,
          reference_device_dpi_x: 96,
          reference_device_dpi_y: 96,
        }),
        ..RenderOptions::default()
      },
    );
    assert_eq!(
      ignored_zero_header,
      extract_metafile_solid_rects(&bytes, Some("image/x-wmf"))
    );
  }

  #[test]
  fn wmf_binary_mask_pair_can_be_lifted_as_a_native_bitmap_layer() {
    // Mask bits are black then white. SRCAND therefore clears the first
    // destination pixel and retains the second; the following SRCPAINT paints
    // red into the cleared pixel while its green second pixel remains hidden.
    let bytes = masked_bitmap_wmf(two_pixel_monochrome_dib(0x40), two_pixel_color_dib());
    let layers = extract_metafile_bitmap_layers(&bytes, Some("image/x-wmf"));
    assert_eq!(layers.len(), 1);
    let layer = &layers[0];
    assert_eq!(
      (layer.x, layer.y, layer.width, layer.height),
      (0.25, 0.0, 0.5, 0.5)
    );
    assert!(!layer.flip_horizontal);
    assert!(!layer.flip_vertical);
    let image = image::load_from_memory_with_format(&layer.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0, 0]);

    let decode = |suppress_bitmap_layers| {
      let decoded = decode_metafile_as_raster_with_options(
        &bytes,
        Some("image/x-wmf"),
        RenderOptions {
          suppress_bitmap_layers,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap();
      image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
        .unwrap()
        .to_rgb8()
    };
    let painted = decode(false);
    let lifted = decode(true);
    assert_eq!(painted.get_pixel(1, 0).0, [255, 0, 0]);
    assert_eq!(painted.get_pixel(2, 0).0, [255, 255, 255]);
    assert_eq!(lifted.get_pixel(1, 0).0, [255, 255, 255]);
    assert_eq!(lifted.get_pixel(2, 0).0, [255, 255, 255]);
  }

  #[test]
  fn wmf_source_copy_can_be_lifted_as_an_opaque_native_bitmap_layer() {
    let bytes = copy_bitmap_wmf();
    let layers = extract_metafile_bitmap_layers(&bytes, Some("image/x-wmf"));
    assert_eq!(layers.len(), 1);
    let layer = &layers[0];
    assert_eq!(
      (layer.x, layer.y, layer.width, layer.height),
      (0.25, 0.0, 0.5, 0.5)
    );
    let image = image::load_from_memory_with_format(&layer.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0]);

    let decode = |suppress_bitmap_layers| {
      let decoded = decode_metafile_as_raster_with_options(
        &bytes,
        Some("image/x-wmf"),
        RenderOptions {
          suppress_bitmap_layers,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap();
      image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
        .unwrap()
        .to_rgb8()
    };
    let painted = decode(false);
    let lifted = decode(true);
    assert_eq!(painted.get_pixel(1, 0).0, [255, 0, 0]);
    assert_eq!(painted.get_pixel(2, 0).0, [0, 255, 0]);
    assert_eq!(lifted.get_pixel(1, 0).0, [255, 255, 255]);
    assert_eq!(lifted.get_pixel(2, 0).0, [255, 255, 255]);
  }

  #[test]
  fn wmf_non_binary_rop_pair_remains_in_destination_replay() {
    let bytes = masked_bitmap_wmf(two_pixel_non_binary_mask_dib(), two_pixel_color_dib());
    assert!(extract_metafile_bitmap_layers(&bytes, Some("image/x-wmf")).is_empty());

    let decode = |suppress_bitmap_layers| {
      decode_metafile_as_raster_with_options(
        &bytes,
        Some("image/x-wmf"),
        RenderOptions {
          suppress_bitmap_layers,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap()
      .data
    };
    assert_eq!(decode(false), decode(true));
  }

  #[test]
  fn wmf_rendering_keeps_valid_output_around_an_unparseable_record() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 2, y: 2 })
        .to_record()
        .unwrap(),
      WmfRecordData::SetPixel(WmfSetPixelRecord {
        color: crate::ColorRef {
          red: 255,
          green: 0,
          blue: 0,
          reserved: 0,
        },
        x: 0,
        y: 0,
      })
      .to_record()
      .unwrap(),
      // META_ESCAPE with an unsupported EscapeFunction. This is a valid raw
      // WMF record retained by compatibility parsing, but has no typed form.
      WmfRecord::new(crate::wmf::WmfRecordFunction::Escape.raw(), vec![0; 4]),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let metafile = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 29,
        number_of_objects: 0,
        max_record_words: 7,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    };

    let bytes = metafile.to_bytes().unwrap();
    let decoded = decode_metafile_as_raster(&bytes, Some("image/x-wmf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();

    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn wmf_dib_pattern_brush_preserves_its_color_table_on_color_output() {
    let mut pattern = bitmap_info(8, 8, 1, 0);
    pattern.extend_from_slice(&[0, 0, 0, 0, 255, 255, 255, 0]);
    for row in 0..8 {
      pattern.extend_from_slice(&[if row % 2 == 0 { 0xAA } else { 0x55 }, 0, 0, 0]);
    }
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 8, y: 8 })
        .to_record()
        .unwrap(),
      WmfRecordData::SetBkColor(WmfColorRecord {
        color: crate::ColorRef {
          red: 255,
          green: 128,
          blue: 255,
          reserved: 0,
        },
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SetTextColor(WmfColorRecord {
        color: crate::ColorRef {
          red: 255,
          green: 255,
          blue: 255,
          reserved: 0,
        },
      })
      .to_record()
      .unwrap(),
      WmfRecordData::DibCreatePatternBrush(WmfDibCreatePatternBrushRecord {
        style: WmfBrushStyle::Pattern.raw(),
        color_usage: DibColorUsage::RgbColors.wmf_raw(),
        target: pattern,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::SelectObject(WmfObjectIndexRecord { index: 0 })
        .to_record()
        .unwrap(),
      WmfRecordData::PatBlt(WmfPatBltRecord {
        raster_operation: 0x00F0_0021,
        height: 8,
        width: 8,
        y_left: 0,
        x_left: 0,
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let metafile = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 1,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    };

    let bytes = metafile.to_bytes().unwrap();
    let decoded = decode_metafile_as_raster(&bytes, Some("image/x-wmf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();

    assert!(
      image.pixels().any(|pixel| pixel.0 == [0, 0, 0]),
      "the first DIB palette entry remains black"
    );
    assert!(
      image.pixels().any(|pixel| pixel.0 == [255, 255, 255]),
      "the second DIB palette entry remains white"
    );
    assert!(
      !image.pixels().any(|pixel| pixel.0 == [255, 128, 255]),
      "color output does not substitute the DC background color"
    );

    let decoded = decode_metafile_as_raster_with_options(
      &bytes,
      Some("image/x-wmf"),
      RenderOptions {
        monochrome_dib_palette_override: Some([[255, 128, 255], [255, 255, 255]]),
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();
    assert!(
      image.pixels().any(|pixel| pixel.0 == [255, 128, 255]),
      "the opt-in realization palette replaces entry zero"
    );
    assert!(
      image.pixels().any(|pixel| pixel.0 == [255, 255, 255]),
      "the opt-in realization palette preserves entry one"
    );
    assert!(
      !image.pixels().any(|pixel| pixel.0 == [0, 0, 0]),
      "the embedded black entry is replaced only for this caller"
    );

    let decoded = decode_metafile_as_raster_with_options(
      &bytes,
      Some("image/x-wmf"),
      RenderOptions {
        monochrome_dib_palette_override: Some([[255, 128, 255], [255, 255, 255]]),
        filter_high_frequency_pattern_brushes: true,
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory_with_format(&decoded.data, image::ImageFormat::Png)
      .unwrap()
      .to_rgb8();
    assert!(
      image.pixels().all(|pixel| pixel.0 == [255, 191, 255]),
      "fixed output box-filters a one-pixel checkerboard before later rescaling"
    );
  }

  #[test]
  fn world_unit_pen_width_uses_the_active_device_transform() {
    let mut data = vec![0; EMF_HEADER_SIZE];
    data[16..20].copy_from_slice(&999i32.to_le_bytes());
    data[20..24].copy_from_slice(&999i32.to_le_bytes());
    let mut state = EmfVectorState::new_with_options(
      &data,
      RenderOptions {
        target_width_px: Some(100),
        target_height_px: Some(100),
        max_pixels: None,
        transparent_background: false,
        background_color: None,
        monochrome_dib_palette_override: None,
        filter_high_frequency_pattern_brushes: false,
        suppress_text: false,
        suppress_solid_pattern_rects: false,
        suppress_bitmap_layers: false,
        wmf_external_header: None,
      },
    )
    .expect("minimal EMF bounds");
    state.world_transform.m11 = 0.5;
    state.world_transform.m22 = 0.5;

    let pen = state.resolve_pen(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width: 100,
      width_space: EmfPenWidthSpace::World,
    });

    assert_eq!(pen.width, 5);
    assert_eq!(pen.width_space, EmfPenWidthSpace::Device);
  }

  #[test]
  fn wmf_pen_width_is_realized_from_only_the_logical_x_scalar() {
    let mut data = vec![0; EMF_HEADER_SIZE];
    data[16..20].copy_from_slice(&2623i32.to_le_bytes());
    data[20..24].copy_from_slice(&2591i32.to_le_bytes());
    let mut state = EmfVectorState::new_with_options(
      &data,
      RenderOptions {
        target_width_px: Some(227),
        target_height_px: Some(225),
        max_pixels: None,
        transparent_background: false,
        background_color: None,
        monochrome_dib_palette_override: None,
        filter_high_frequency_pattern_brushes: false,
        suppress_text: false,
        suppress_solid_pattern_rects: false,
        suppress_bitmap_layers: false,
        wmf_external_header: None,
      },
    )
    .expect("minimal EMF bounds");
    state.window_ext_x = 2624;
    state.window_ext_y = 2592;
    state.viewport_ext_x = 2624;
    state.viewport_ext_y = 2592;

    let (width, width_space) = wmf_pen_width(16);
    let pen = state.resolve_pen(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width,
      width_space,
    });
    assert_eq!(pen.width, 1, "16 logical x units map to 1.38 pixels");

    state.output_scale_y *= 20.0;
    let y_scaled_pen = state.resolve_pen(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width,
      width_space,
    });
    assert_eq!(
      y_scaled_pen.width, 1,
      "y-only scaling does not change a WMF pen width"
    );

    state.output_scale_x = 0.2;
    let x_scaled_pen = state.resolve_pen(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width,
      width_space,
    });
    assert_eq!(x_scaled_pen.width, 3);

    let (hairline_width, hairline_space) = wmf_pen_width(0);
    state.output_scale_x = 20.0;
    let hairline = state.resolve_pen(EmfPen {
      color: EmfColor { r: 0, g: 0, b: 0 },
      alpha: u8::MAX,
      width: hairline_width,
      width_space: hairline_space,
    });
    assert_eq!(hairline.width, 1);
    assert_eq!(hairline.width_space, EmfPenWidthSpace::Device);
  }

  #[test]
  fn emf_null_pen_disables_polygon_outlines() {
    let pen = EmfPen {
      color: EmfColor {
        r: 255,
        g: 255,
        b: 255,
      },
      alpha: u8::MAX,
      width: 1,
      width_space: EmfPenWidthSpace::Device,
    };
    assert!(emf_pen_from_style(EmrPenLineStyle::Solid.raw(), pen).is_some());
    assert!(emf_pen_from_style(EmrPenLineStyle::Null.raw(), pen).is_none());

    let mut data = vec![0; EMF_HEADER_SIZE];
    data[16..20].copy_from_slice(&9i32.to_le_bytes());
    data[20..24].copy_from_slice(&9i32.to_le_bytes());
    let mut state = EmfVectorState::new_with_options(&data, RenderOptions::default())
      .expect("minimal EMF bounds");
    state.pens.insert(7, None);
    state.select_object(7);
    assert!(state.current_pen.is_none());
  }

  #[test]
  fn emf_fixed_ext_create_null_pen_has_no_required_style_entry() {
    let mut ext_pen = Vec::with_capacity(44);
    ext_pen.extend_from_slice(&2u32.to_le_bytes());
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // offBmi
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // cbBmi
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // offBits
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // cbBits
    ext_pen.extend_from_slice(&EmrPenLineStyle::Null.raw().to_le_bytes());
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // Width
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // BS_SOLID (ignored by PS_NULL)
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // ColorRef
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // BrushHatch
    ext_pen.extend_from_slice(&0u32.to_le_bytes()); // NumStyleEntries
    assert_eq!(ext_pen.len() + EMF_RECORD_HEADER_SIZE, 52);

    let metafile = metafile_with_header_bounds(
      9,
      9,
      vec![
        create_solid_brush_record(1, 0x0000_00ff),
        select_object_record(1),
        EmfRecord::new(super::EMR_EXT_CREATE_PEN, ext_pen),
        select_object_record(2),
        triangle_polygon16_record(),
      ],
    );
    let decoded = decode_metafile_as_raster(&metafile, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();

    assert!(image.pixels().any(|pixel| pixel.0 == [255, 0, 0]));
    assert!(
      image.pixels().all(|pixel| pixel.0 != [0, 0, 0]),
      "a 52-byte PS_NULL pen must not fall back to the default black pen"
    );

    let scene = extract_metafile_vector_scene(&metafile, Some("image/x-emf"))
      .unwrap()
      .expect("solid fill scene");
    assert_eq!(scene.fills.len(), 1);
    assert_eq!(scene.fills[0].color, [255, 0, 0]);
    assert_eq!(scene.fills[0].fill_rule, MetafileVectorFillRule::Alternate);
    assert_eq!(scene.fills[0].subpaths.len(), 1);
  }

  #[test]
  fn metafile_vector_scene_rejects_a_visible_polygon_pen() {
    let metafile = metafile_with_header_bounds(
      9,
      9,
      vec![
        create_solid_brush_record(1, 0x0000_00ff),
        select_object_record(1),
        triangle_polygon16_record(),
      ],
    );

    assert_eq!(
      extract_metafile_vector_scene(&metafile, Some("image/x-emf")).unwrap(),
      None,
      "a fill-only scene must not silently discard the default black outline"
    );
  }

  #[test]
  fn emf_decomposed_scene_accepts_only_a_line_fully_covered_by_lifted_patcopy() {
    const GRID_COLOR: u32 = 0x00dd_dcda;
    let metafile = metafile_with_header_bounds(
      9,
      9,
      vec![
        create_cosmetic_pen_record(1, GRID_COLOR),
        create_solid_brush_record(2, GRID_COLOR),
        select_object_record(1),
        select_object_record(2),
        move_to_ex_record(0, 4),
        line_to_record(8, 4),
        select_object_record(crate::emf::EmrStockObject::BlackPen.raw()),
        source_less_bit_blt_rect_record(
          0,
          4,
          8,
          1,
          WmfTernaryRasterOperationCode::PATCOPY.canonical_raw(),
        ),
      ],
    );

    assert_eq!(
      extract_metafile_vector_scene(&metafile, Some("image/x-emf")).unwrap(),
      None,
      "ordinary playback must retain the visible cosmetic line and PATCOPY"
    );
    assert_eq!(
      extract_metafile_vector_scene_with_options(
        &metafile,
        Some("image/x-emf"),
        RenderOptions {
          suppress_solid_pattern_rects: true,
          ..RenderOptions::default()
        },
      )
      .unwrap(),
      Some(MetafileVectorScene::default()),
      "the later opaque PATCOPY is the complete visible replacement for the matching line"
    );

    for (brush_color, rect_width, rect_height) in
      [(0x00dd_dcdb, 8, 1), (GRID_COLOR, 7, 1), (GRID_COLOR, 8, 2)]
    {
      let counterexample = metafile_with_header_bounds(
        9,
        9,
        vec![
          create_cosmetic_pen_record(1, GRID_COLOR),
          create_solid_brush_record(2, brush_color),
          select_object_record(1),
          select_object_record(2),
          move_to_ex_record(0, 4),
          line_to_record(8, 4),
          source_less_bit_blt_rect_record(
            0,
            4,
            rect_width,
            rect_height,
            WmfTernaryRasterOperationCode::PATCOPY.canonical_raw(),
          ),
        ],
      );
      assert_eq!(
        extract_metafile_vector_scene_with_options(
          &counterexample,
          Some("image/x-emf"),
          RenderOptions {
            suppress_solid_pattern_rects: true,
            ..RenderOptions::default()
          },
        )
        .unwrap(),
        None,
        "a mismatched color or rectangle cannot prove that PATCOPY covers the line"
      );
    }
  }

  #[test]
  fn emf_binary_raster_operations_follow_rop2_boolean_semantics() {
    let pen = EmfColor {
      r: 0b1010_1010,
      g: 0b1100_1100,
      b: 0b1111_0000,
    };
    let destination = EmfColor {
      r: 0b1111_0000,
      g: 0b1010_1010,
      b: 0b1100_1100,
    };

    assert_eq!(
      apply_binary_raster_operation(pen, destination, WmfBinaryRasterOperation::XorPen),
      EmfColor {
        r: destination.r ^ pen.r,
        g: destination.g ^ pen.g,
        b: destination.b ^ pen.b,
      }
    );
    assert_eq!(
      apply_binary_raster_operation(pen, destination, WmfBinaryRasterOperation::CopyPen),
      pen
    );
    assert_eq!(
      apply_binary_raster_operation(pen, destination, WmfBinaryRasterOperation::Nop),
      destination
    );
    assert_eq!(
      apply_binary_raster_operation(pen, destination, WmfBinaryRasterOperation::Black),
      EmfColor { r: 0, g: 0, b: 0 }
    );
    assert_eq!(
      apply_binary_raster_operation(pen, destination, WmfBinaryRasterOperation::White),
      EmfColor {
        r: u8::MAX,
        g: u8::MAX,
        b: u8::MAX,
      }
    );
  }

  #[test]
  fn emf_alpha_blend_scales_premultiplied_channels_only_once() {
    assert_eq!(
      gdi_alpha_blend_color(
        EmfColor {
          r: 255,
          g: 255,
          b: 255,
        },
        // A half-transparent red source is stored premultiplied as R=128.
        EmfColor { r: 128, g: 0, b: 0 },
        Some(128),
        128,
      ),
      EmfColor {
        r: 255,
        g: 191,
        b: 191,
      }
    );
  }

  fn stretch_record(bitmap_info: Vec<u8>, bitmap_bits: Vec<u8>) -> EmfRecord {
    stretch_dibits_record(bitmap_info, bitmap_bits, 0x00CC_0020)
  }

  fn stretch_dibits_record(
    bitmap_info: Vec<u8>,
    bitmap_bits: Vec<u8>,
    raster_operation: u32,
  ) -> EmfRecord {
    EmfRecordData::StretchDiBits(EmrStretchDiBits {
      bounds: RectL::default(),
      dest: crate::PointL { x: 0, y: 0 },
      source: BitmapSourceBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      color_usage: DibColorUsage::RgbColors.raw(),
      raster_operation,
      dest_size: SizeL { cx: 2, cy: 2 },
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info,
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits,
      },
      padding: Vec::new(),
    })
    .to_record()
    .unwrap()
  }

  fn stretch_blt_record(
    bitmap_info: Vec<u8>,
    bitmap_bits: Vec<u8>,
    raster_operation: u32,
  ) -> EmfRecord {
    EmfRecordData::StretchBlt(EmrStretchBlt {
      bounds: RectL::default(),
      dest: PointL { x: 0, y: 0 },
      dest_size: SizeL { cx: 2, cy: 2 },
      raster_operation,
      source: PointL { x: 0, y: 0 },
      xform_source: XForm {
        m11: 1.0,
        m22: 1.0,
        ..XForm::default()
      },
      background_color_source: ColorRef::default(),
      color_usage: DibColorUsage::RgbColors.raw(),
      source_size: SizeL { cx: 2, cy: 2 },
      bitmap: Some(EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info,
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits,
      }),
      padding: Vec::new(),
    })
    .to_record()
    .unwrap()
  }

  fn alpha_blend_record(source_constant_alpha: u8) -> EmfRecord {
    EmfRecordData::AlphaBlend(EmrAlphaBlend {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 1,
        bottom: 0,
      },
      dest: PointL { x: 0, y: 0 },
      dest_size: SizeL { cx: 2, cy: 1 },
      blend_function: EmrBlendFunction {
        blend_operation: 0,
        // [MS-EMF] says BlendFlags MUST be ignored. Adobe's real OLE icon
        // preview sets this reserved byte to 0x80, so keep the counterexample
        // in the renderer test instead of normalizing it away.
        blend_flags: 0x80,
        source_constant_alpha,
        alpha_format: EmrAlphaFormat::SourceAlpha.raw(),
      },
      source: PointL { x: 0, y: 0 },
      xform_source: XForm {
        m11: 1.0,
        m22: 1.0,
        ..XForm::default()
      },
      background_color_source: ColorRef::default(),
      color_usage: DibColorUsage::RgbColors.raw(),
      source_size: SizeL { cx: 2, cy: 1 },
      bitmap: Some(EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info: bitmap_info(2, 1, 32, BI_RGB),
        undefined_space_before_bitmap_bits: Vec::new(),
        // Bottom-up BGRA: half-transparent premultiplied red, then a
        // quarter-transparent premultiplied blue.
        bitmap_bits: vec![0, 0, 128, 128, 64, 0, 0, 64],
      }),
      padding: Vec::new(),
    })
    .to_record()
    .unwrap()
  }

  fn create_solid_brush_record(object_id: u32, color_ref: u32) -> EmfRecord {
    create_brush_record(object_id, WmfBrushStyle::Solid, color_ref)
  }

  fn create_cosmetic_pen_record(object_id: u32, color_ref: u32) -> EmfRecord {
    let mut data = Vec::with_capacity(20);
    data.extend_from_slice(&object_id.to_le_bytes());
    data.extend_from_slice(&EmrPenLineStyle::Solid.raw().to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&color_ref.to_le_bytes());
    EmfRecord::new(super::EMR_CREATE_PEN, data)
  }

  fn create_brush_record(object_id: u32, brush_style: WmfBrushStyle, color_ref: u32) -> EmfRecord {
    let mut data = Vec::with_capacity(16);
    data.extend_from_slice(&object_id.to_le_bytes());
    data.extend_from_slice(&u32::from(brush_style.raw()).to_le_bytes());
    data.extend_from_slice(&color_ref.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    EmfRecord::new(super::EMR_CREATE_BRUSH_INDIRECT, data)
  }

  fn select_object_record(object_id: u32) -> EmfRecord {
    EmfRecord::new(super::EMR_SELECT_OBJECT, object_id.to_le_bytes().to_vec())
  }

  fn line_to_record(x: i32, y: i32) -> EmfRecord {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    EmfRecord::new(super::EMR_LINE_TO, data)
  }

  fn triangle_polygon16_record() -> EmfRecord {
    let mut data = vec![0; 20];
    data[0..4].copy_from_slice(&2i32.to_le_bytes());
    data[4..8].copy_from_slice(&2i32.to_le_bytes());
    data[8..12].copy_from_slice(&7i32.to_le_bytes());
    data[12..16].copy_from_slice(&7i32.to_le_bytes());
    data[16..20].copy_from_slice(&3u32.to_le_bytes());
    for (x, y) in [(2i16, 2i16), (7, 2), (4, 7)] {
      data.extend_from_slice(&x.to_le_bytes());
      data.extend_from_slice(&y.to_le_bytes());
    }
    EmfRecord::new(super::EMR_POLYGON16, data)
  }

  fn source_less_bit_blt_record(raster_operation: u32) -> EmfRecord {
    source_less_bit_blt_rect_record(0, 0, 1, 1, raster_operation)
  }

  fn source_less_bit_blt_rect_record(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    raster_operation: u32,
  ) -> EmfRecord {
    EmfRecordData::BitBlt(EmrBitBlt {
      bounds: RectL {
        left: x,
        top: y,
        right: x.saturating_add(width),
        bottom: y.saturating_add(height),
      },
      dest: PointL { x, y },
      dest_size: SizeL {
        cx: width,
        cy: height,
      },
      raster_operation,
      source: PointL::default(),
      xform_source: XForm {
        m11: 1.0,
        m22: 1.0,
        ..XForm::default()
      },
      background_color_source: ColorRef::default(),
      color_usage: DibColorUsage::RgbColors.raw(),
      bitmap: None,
      padding: Vec::new(),
    })
    .to_record()
    .unwrap()
  }

  fn metafile_with(record: EmfRecord) -> Vec<u8> {
    metafile_with_records(vec![record])
  }

  fn metafile_with_records(records: Vec<EmfRecord>) -> Vec<u8> {
    metafile_with_header_bounds(1, 1, records)
  }

  fn metafile_with_header_bounds(right: i32, bottom: i32, records: Vec<EmfRecord>) -> Vec<u8> {
    let mut all_records = Vec::with_capacity(records.len() + 2);
    all_records.push(header_record(right, bottom));
    all_records.extend(records);
    all_records.push(eof_record());
    EmfMetafile {
      records: all_records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap()
  }

  fn set_pixel_record(x: i32, y: i32, color_ref: u32) -> EmfRecord {
    let mut data = Vec::new();
    data.extend_from_slice(&x.to_le_bytes());
    data.extend_from_slice(&y.to_le_bytes());
    data.extend_from_slice(&color_ref.to_le_bytes());
    EmfRecord::new(super::EMR_SET_PIXEL_V, data)
  }

  fn emf_plus_comment_record(records: Vec<EmfPlusRecord>) -> EmfRecord {
    let stream = EmfPlusStream {
      records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap();
    let mut data = Vec::with_capacity(8 + stream.len());
    data.extend_from_slice(&(u32::try_from(stream.len()).unwrap() + 4).to_le_bytes());
    data.extend_from_slice(&EMR_COMMENT_EMFPLUS.to_le_bytes());
    data.extend_from_slice(&stream);
    EmfRecord::new(super::EMR_COMMENT, data)
  }

  fn emf_plus_record(data: EmfPlusRecordData<'_>) -> EmfPlusRecord {
    EmfPlusRecord::from_data(&data, EmfPlusRecordFlags::empty()).unwrap()
  }

  fn emf_plus_header_comment(dual: bool) -> EmfRecord {
    let data = EmfPlusRecordData::Header(EmfPlusHeaderData {
      graphics_version: EmfPlusGraphicsVersion::from_graphics_version(
        EmfPlusGraphicsVersionValue::Version1_1,
      ),
      emf_plus_flags: 0,
      logical_dpi_x: 96,
      logical_dpi_y: 96,
    });
    let flags = EmfPlusRecordFlags::from_bits_retain(u16::from(dual));
    emf_plus_comment_record(vec![EmfPlusRecord::from_data(&data, flags).unwrap()])
  }

  #[test]
  fn polygon_scanlines_are_limited_to_the_visible_vertical_bounds() {
    let points = [(2.0, 10.0), (5.0, 10.0), (5.0, 12.0), (2.0, 12.0)];
    let mut spans = Vec::new();
    visit_polygon_scanline_spans(&points, 20, 10_000, |y, start, end| {
      spans.push((y, start, end));
    });

    assert_eq!(spans, [(10, 2, 5), (11, 2, 5)]);

    spans.clear();
    visit_polygon_scanline_spans(&points, 20, 8, |y, start, end| {
      spans.push((y, start, end));
    });
    assert!(spans.is_empty());
  }

  #[test]
  fn adjacent_slanted_polygon_bands_use_a_half_open_shared_edge() {
    let left = [(0.0, 0.0), (2.0, 0.0), (4.0, 2.0), (2.0, 2.0)];
    let right = [(2.0, 0.0), (4.0, 0.0), (6.0, 2.0), (4.0, 2.0)];
    let mut left_spans = Vec::new();
    let mut right_spans = Vec::new();
    visit_polygon_scanline_spans(&left, 8, 2, |y, start, end| {
      left_spans.push((y, start, end));
    });
    visit_polygon_scanline_spans(&right, 8, 2, |y, start, end| {
      right_spans.push((y, start, end));
    });

    assert_eq!(left_spans[0], (0, 0, 2));
    assert_eq!(right_spans[0], (0, 2, 4));
    assert_eq!(left_spans[0].2, right_spans[0].1);
  }

  #[test]
  fn axis_aligned_polygon_clip_uses_the_same_pixel_center_bounds() {
    let points = [(2.2, 10.8), (5.2, 10.8), (5.2, 13.2), (2.2, 13.2)];
    assert_eq!(
      axis_aligned_clip_rect(&points, 20, 20),
      Some((2, 11, 6, 13))
    );

    let rotated = [(2.0, 3.0), (4.0, 2.0), (5.0, 4.0), (3.0, 5.0)];
    assert_eq!(axis_aligned_clip_rect(&rotated, 20, 20), None);
  }

  #[test]
  fn rectangle_clip_intersection_preserves_empty_regions() {
    assert_eq!(intersect_rects((1, 2, 8, 9), (4, 0, 10, 6)), (4, 2, 8, 6));
    assert_eq!(intersect_rects((1, 1, 2, 2), (4, 4, 5, 5)), (4, 4, 4, 4));
  }

  #[test]
  fn decode_emf_embedded_png_bitmap() {
    let bits = vec![0x89, b'P', b'N', b'G'];
    let mut info = bitmap_info(2, 2, 0, BI_PNG);
    info[20..24].copy_from_slice(&(bits.len() as u32).to_le_bytes());
    let emf = metafile_with(stretch_record(info, bits));

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    assert_eq!(decoded.data, [0x89, b'P', b'N', b'G']);
  }

  #[test]
  fn decode_emf_bi_rgb_bitmap_as_png() {
    let bits = vec![
      0, 0, 255, 0, 255, 0, 0, 0, // bottom row: red, green, padding
      255, 0, 0, 255, 255, 255, 0, 0, // top row: blue, white, padding
    ];
    let emf = metafile_with(stretch_record(bitmap_info(2, 2, 24, BI_RGB), bits));

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    assert!(decoded.data.starts_with(&[0x89, b'P', b'N', b'G']));
  }

  #[test]
  fn emf_negative_window_extent_keeps_stretch_dibits_on_canvas() {
    let bits = vec![
      0, 0, 255, 0, 255, 0, 0, 0, // bottom row: red, green, padding
      255, 0, 0, 0, 255, 255, 0, 0, // top row: blue, yellow, padding
    ];
    let stretch = EmfRecordData::StretchDiBits(EmrStretchDiBits {
      bounds: RectL {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
      },
      dest: PointL { x: 0, y: 0 },
      source: BitmapSourceBounds {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
      },
      color_usage: DibColorUsage::RgbColors.raw(),
      raster_operation: 0x00CC_0020,
      dest_size: SizeL { cx: 2, cy: -2 },
      bitmap: EmrBitmapBuffer {
        undefined_space_before_bitmap_info: Vec::new(),
        bitmap_info: bitmap_info(2, 2, 24, BI_RGB),
        undefined_space_before_bitmap_bits: Vec::new(),
        bitmap_bits: bits,
      },
      padding: Vec::new(),
    })
    .to_record()
    .unwrap();
    let emf = metafile_with_header_bounds(
      1,
      1,
      vec![
        set_map_mode_record(EmrMapMode::Anisotropic),
        extent_record(super::EMR_SET_VIEWPORT_EXT_EX, 2, 2),
        extent_record(super::EMR_SET_WINDOW_EXT_EX, 2, -2),
        stretch,
      ],
    );

    let decoded =
      decode_vector_emf_as_png(&emf, RenderOptions::default(), GdiTextSurface::Color).unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();

    assert_eq!(image.dimensions(), (2, 2));
    assert!(image.pixels().all(|pixel| pixel.0 != [255, 255, 255]));
  }

  #[test]
  fn decode_emf_replays_bitmap_with_later_vector_records() {
    let bits = vec![
      255, 255, 255, 255, 255, 255, 0, 0, // bottom row: white, white, padding
      255, 255, 255, 255, 255, 255, 0, 0, // top row: white, white, padding
    ];
    let emf = metafile_with_records(vec![
      stretch_record(bitmap_info(2, 2, 24, BI_RGB), bits),
      set_pixel_record(0, 0, 0x0000_00ff),
    ]);

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn decode_emf_replays_alpha_blend_with_per_pixel_alpha() {
    let emf = metafile_with_header_bounds(1, 0, vec![alpha_blend_record(255)]);
    let decoded = decode_metafile_as_raster_with_options(
      &emf,
      Some("image/x-emf"),
      RenderOptions {
        target_width_px: Some(2),
        target_height_px: Some(1),
        max_pixels: Some(2),
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();

    assert_eq!(image.dimensions(), (2, 1));
    assert_eq!(image.get_pixel(0, 0).0, [255, 127, 127]);
    assert_eq!(image.get_pixel(1, 0).0, [191, 191, 255]);
  }

  #[test]
  fn decode_emf_replays_source_less_pattern_bit_blt() {
    let brush_id = 1;
    let emf = metafile_with_records(vec![
      create_solid_brush_record(brush_id, 0x0033_2211),
      select_object_record(brush_id),
      source_less_bit_blt_record(0x00F0_0021),
    ]);

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();

    assert_eq!(image.get_pixel(0, 0).0, [0x11, 0x22, 0x33]);
  }

  #[test]
  fn emf_solid_patcopy_rects_can_be_lifted_without_suppressing_other_graphics() {
    let brush_id = 1;
    let emf = metafile_with_header_bounds(
      7,
      7,
      vec![
        create_solid_brush_record(brush_id, 0x0000_00ff),
        select_object_record(brush_id),
        source_less_bit_blt_rect_record(2, 3, 4, 2, 0x00F0_0021),
        set_pixel_record(0, 0, 0x00ff_0000),
      ],
    );

    assert_eq!(
      extract_metafile_solid_rects(&emf, Some("image/x-emf")),
      vec![MetafileSolidRect {
        x: 0.25,
        y: 0.375,
        width: 0.5,
        height: 0.25,
        color: [255, 0, 0],
      }]
    );

    let decode = |suppress_solid_pattern_rects| {
      let decoded = decode_metafile_as_raster_with_options(
        &emf,
        Some("image/x-emf"),
        RenderOptions {
          suppress_solid_pattern_rects,
          ..RenderOptions::default()
        },
      )
      .unwrap()
      .unwrap();
      image::load_from_memory(&decoded.data).unwrap().to_rgb8()
    };
    let painted = decode(false);
    let lifted = decode(true);

    assert_eq!(painted.get_pixel(2, 3).0, [255, 0, 0]);
    assert_eq!(lifted.get_pixel(2, 3).0, [255, 255, 255]);
    assert_eq!(painted.get_pixel(0, 0).0, [0, 0, 255]);
    assert_eq!(lifted.get_pixel(0, 0).0, [0, 0, 255]);
  }

  #[test]
  fn emf_solid_rect_lifting_rejects_pattern_brushes_and_other_rops() {
    let brush_id = 1;
    let patterned = metafile_with_records(vec![
      create_brush_record(brush_id, WmfBrushStyle::Hatched, 0x0000_00ff),
      select_object_record(brush_id),
      source_less_bit_blt_record(0x00F0_0021),
    ]);
    let other_rop = metafile_with_records(vec![
      create_solid_brush_record(brush_id, 0x0000_00ff),
      select_object_record(brush_id),
      source_less_bit_blt_record(WmfTernaryRasterOperationCode::PATINVERT.canonical_raw()),
    ]);

    assert!(extract_metafile_solid_rects(&patterned, Some("image/x-emf")).is_empty());
    assert!(extract_metafile_solid_rects(&other_rop, Some("image/x-emf")).is_empty());

    let decoded = decode_metafile_as_raster_with_options(
      &patterned,
      Some("image/x-emf"),
      RenderOptions {
        suppress_solid_pattern_rects: true,
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn emf_plus_playback_skips_classic_fallback_in_only_and_dual_modes() {
    for dual in [false, true] {
      let emf = metafile_with_records(vec![
        emf_plus_header_comment(dual),
        set_pixel_record(0, 0, 0x0000_00ff),
      ]);

      let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
        .unwrap()
        .unwrap();
      let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
      assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255]);
    }
  }

  #[test]
  fn emf_plus_get_dc_borrows_a_fresh_classic_device_context() {
    let emf = metafile_with_records(vec![
      emf_plus_header_comment(true),
      emf_plus_comment_record(vec![
        emf_plus_record(EmfPlusRecordData::SetWorldTransform(XForm {
          m11: 1.0,
          m12: 0.0,
          m21: 0.0,
          m22: 1.0,
          dx: 1.0,
          dy: 1.0,
        })),
        emf_plus_record(EmfPlusRecordData::GetDc),
      ]),
      set_pixel_record(0, 0, 0x0000_00ff),
      emf_plus_comment_record(vec![emf_plus_record(EmfPlusRecordData::ResetClip)]),
      set_pixel_record(1, 1, 0x00ff_0000),
    ]);

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
    assert_eq!(image.get_pixel(1, 1).0, [255, 255, 255]);
  }

  #[test]
  fn emf_plus_infinite_region_uses_the_playback_surface_not_a_fixed_rectangle() {
    let region = EmfPlusRegionObject {
      version: EmfPlusGraphicsVersion::from_graphics_version(
        EmfPlusGraphicsVersionValue::Version1_1,
      ),
      region_node_count: 0,
      region_nodes: EmfPlusRegionNodeDataType::Infinite
        .raw()
        .to_le_bytes()
        .to_vec(),
    };
    let region = emf_plus_region_object(&region).unwrap();
    assert!(matches!(region, EmfPlusRenderRegion::Infinite));

    let data = metafile_with_records(Vec::new());
    let mut state = EmfVectorState::new_with_options(&data, RenderOptions::default()).unwrap();
    state.set_clip_rect_device((0, 0, 1, 1), 0);
    state.set_clip_region(&region, 0);
    assert_eq!(state.clip_rect, None);
    assert_eq!(state.clip_mask, None);

    let red = EmfColor { r: 255, g: 0, b: 0 };
    state.set_pixel(1, 1, red);
    assert_eq!(state.pixel(1, 1), Some(red));
  }

  #[test]
  fn emf_plus_region_boolean_operations_distinguish_empty_from_infinite() {
    let data = metafile_with_records(Vec::new());
    let state = EmfVectorState::new_with_options(&data, RenderOptions::default()).unwrap();
    let finite = vec![true, false, false, true];

    assert_eq!(
      state.combine_region_masks(None, Some(finite.clone()), 3),
      Some(vec![false, true, true, false])
    );
    assert_eq!(
      state.combine_region_masks(None, Some(finite.clone()), 4),
      Some(vec![false, true, true, false])
    );
    assert_eq!(
      state.combine_region_masks(None, Some(finite), 5),
      Some(vec![false; 4])
    );
  }

  #[test]
  fn emf_plus_restore_uses_stack_index_instead_of_the_latest_state() {
    let data = metafile_with_records(Vec::new());
    let mut state = EmfVectorState::new_with_options(&data, RenderOptions::default()).unwrap();
    state.save_emf_plus_state(7, false);
    state.set_clip_rect_device((0, 0, 1, 1), 0);
    state.save_emf_plus_state(9, false);
    state.set_clip_rect_device((1, 1, 2, 2), 0);

    state.restore_emf_plus_state(7, false);

    assert_eq!(state.clip_rect, None);
    assert_eq!(state.clip_mask, None);
    assert!(state.emf_plus_saved_states.is_empty());
  }

  #[test]
  fn emf_plus_rendering_interprets_negative_start_angles_modulo_360() {
    let data = EmfPlusRecordData::FillPie(EmfPlusFillPieData {
      brush: EmfPlusBrushRef::Color(crate::EmfPlusArgb {
        blue: 189,
        green: 129,
        red: 79,
        alpha: 255,
      }),
      start_angle: 30.0,
      sweep_angle: -60.0,
      rect: crate::EmfPlusRect::Float(crate::RectF {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
      }),
    });
    let mut record = EmfPlusRecord::from_data(&data, EmfPlusRecordFlags::empty()).unwrap();
    record.data[4..8].copy_from_slice(&(-30.0f32).to_le_bytes());

    assert!(record.parse_data().is_err());
    assert!(matches!(
      record.parse_data_relaxed().unwrap(),
      EmfPlusRecordData::FillPie(EmfPlusFillPieData {
        start_angle: -30.0,
        ..
      })
    ));
    let negative = arc_segment_points(0, 0, 100, 100, -30.0, -60.0, true)
      .into_iter()
      .map(|point| (point.x, point.y))
      .collect::<Vec<_>>();
    let normalized = arc_segment_points(0, 0, 100, 100, 330.0, -60.0, true)
      .into_iter()
      .map(|point| (point.x, point.y))
      .collect::<Vec<_>>();
    assert_eq!(negative, normalized);
  }

  #[test]
  fn decode_emf_replays_stretch_blt_mask_and_source_rops() {
    let mut mask_info = bitmap_info(2, 2, 1, BI_RGB);
    mask_info.extend_from_slice(&[
      0, 0, 0, 0, // black
      255, 255, 255, 0, // white
    ]);
    let mask_bits = vec![
      0, 0, 0, 0, // bottom row: black, black, padding
      0, 0, 0, 0, // top row: black, black, padding
    ];
    let source_bits = vec![
      0, 0, 255, 0, 0, 0, 255, 0, // bottom row: red, red
      0, 0, 255, 0, 0, 0, 255, 0, // top row: red, red
    ];
    let emf = metafile_with_records(vec![
      stretch_blt_record(mask_info, mask_bits, 0x0088_00C6),
      stretch_blt_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0066_0046),
    ]);

    let decoded = decode_metafile_as_raster(&emf, Some("image/x-emf"))
      .unwrap()
      .unwrap();
    assert_eq!(decoded.content_type, "image/png");
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [255, 0, 0]);
  }

  #[test]
  fn decode_emf_combines_stretch_blt_mask_with_stretch_dibits_source() {
    let mut mask_info = bitmap_info(2, 2, 1, BI_RGB);
    mask_info.extend_from_slice(&[
      0, 0, 0, 0, // black
      255, 255, 255, 0, // white
    ]);
    let mask_bits = vec![
      0x40, 0, 0, 0, // bottom row: black, white, padding
      0x40, 0, 0, 0, // top row: black, white, padding
    ];
    let source_bits = vec![
      255, 0, 0, 0, 0, 0, 0, 0, // bottom row: blue, black
      255, 0, 0, 0, 0, 0, 0, 0, // top row: blue, black
    ];
    let emf = metafile_with_records(vec![
      stretch_blt_record(mask_info, mask_bits, 0x0088_00C6),
      stretch_dibits_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0066_0046),
    ]);
    assert!(emf_uses_binary_coverage_surface(&emf).unwrap());

    let decoded = decode_metafile_as_raster_with_options(
      &emf,
      Some("image/x-emf"),
      RenderOptions {
        target_width_px: Some(4),
        target_height_px: Some(2),
        transparent_background: true,
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgba8();

    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 0, 191, 255]);
    assert_eq!(image.get_pixel(2, 0).0, [0, 0, 128, 255]);
    assert_eq!(image.get_pixel(3, 0).0, [0, 0, 64, 255]);
  }

  #[test]
  fn decode_emf_blt_composes_against_the_caller_background() {
    let mut mask_info = bitmap_info(2, 2, 1, BI_RGB);
    mask_info.extend_from_slice(&[
      0, 0, 0, 0, // black
      255, 255, 255, 0, // white
    ]);
    let mask_bits = vec![
      0x40, 0, 0, 0, // bottom row: black, white, padding
      0x40, 0, 0, 0, // top row: black, white, padding
    ];
    let source_bits = vec![
      255, 0, 0, 0, 0, 0, 0, 0, // bottom row: blue, black
      255, 0, 0, 0, 0, 0, 0, 0, // top row: blue, black
    ];
    let emf = metafile_with_records(vec![
      stretch_blt_record(mask_info, mask_bits, 0x0088_00C6),
      stretch_blt_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0066_0046),
    ]);

    let decoded = decode_metafile_as_raster_with_options(
      &emf,
      Some("image/x-emf"),
      RenderOptions {
        background_color: Some([255, 0, 0]),
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgb8();
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [255, 0, 0]);
  }

  #[test]
  fn decode_emf_blt_reconstructs_a_transparent_destination() {
    let mut mask_info = bitmap_info(2, 2, 1, BI_RGB);
    mask_info.extend_from_slice(&[
      0, 0, 0, 0, // black
      255, 255, 255, 0, // white
    ]);
    let mask_bits = vec![
      0x40, 0, 0, 0, // bottom row: black, white, padding
      0x40, 0, 0, 0, // top row: black, white, padding
    ];
    let source_bits = vec![
      255, 0, 0, 0, 0, 0, 0, 0, // bottom row: blue, black
      255, 0, 0, 0, 0, 0, 0, 0, // top row: blue, black
    ];
    let emf = metafile_with_records(vec![
      stretch_blt_record(mask_info, mask_bits, 0x0088_00C6),
      stretch_blt_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0066_0046),
    ]);

    let decoded = decode_metafile_as_raster_with_options(
      &emf,
      Some("image/x-emf"),
      RenderOptions {
        transparent_background: true,
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgba8();
    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 0, 0, 0]);
  }

  #[test]
  fn gdi_plus_metafile_bilinear_sampling_uses_endpoint_over_target_extent() {
    let image = RasterPixels {
      width: 32,
      height: 1,
      rgb: (0..32)
        .flat_map(|index| {
          let value = index * 8;
          [value, value, value]
        })
        .collect(),
    };
    let samples = (0..66)
      .map(|x| gdi_plus_bilinear_raster_color(&image, x, 0, 66, 1).r)
      .collect::<Vec<_>>();

    assert_eq!(&samples[..5], &[0, 4, 8, 11, 15]);
    assert_eq!(samples[65], 244);
  }

  #[test]
  fn resized_masked_blt_keeps_filtered_source_color_fringe() {
    let mut mask_info = bitmap_info(2, 2, 1, BI_RGB);
    mask_info.extend_from_slice(&[
      0, 0, 0, 0, // black
      255, 255, 255, 0, // white
    ]);
    let mask_bits = vec![
      0x40, 0, 0, 0, // bottom row: black, white, padding
      0x40, 0, 0, 0, // top row: black, white, padding
    ];
    let source_bits = vec![
      255, 0, 0, 0, 0, 0, 0, 0, // bottom row: blue, black
      255, 0, 0, 0, 0, 0, 0, 0, // top row: blue, black
    ];
    let emf = metafile_with_records(vec![
      stretch_blt_record(mask_info, mask_bits, 0x0088_00C6),
      stretch_blt_record(bitmap_info(2, 2, 32, BI_RGB), source_bits, 0x0066_0046),
    ]);

    let decoded = decode_metafile_as_raster_with_options(
      &emf,
      Some("image/x-emf"),
      RenderOptions {
        target_width_px: Some(4),
        target_height_px: Some(2),
        transparent_background: true,
        ..RenderOptions::default()
      },
    )
    .unwrap()
    .unwrap();
    let image = image::load_from_memory(&decoded.data).unwrap().to_rgba8();

    assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 0, 191, 255]);
    assert_eq!(image.get_pixel(2, 0).0, [0, 0, 128, 255]);
    assert_eq!(image.get_pixel(3, 0).0, [0, 0, 64, 255]);
  }

  #[test]
  fn cleartype_box_decimation_displaces_rgb_windows_over_six_samples() {
    let (left, width, coverage) = cleartype_box_decimate(&[255; 6], 6, 1, 0);

    assert_eq!(left, -1);
    assert_eq!(width, 3);
    assert_eq!(
      coverage,
      [[0, 0, 85], [170, 255, 170], [85, 0, 0]],
      "the one-pixel box is centered independently on the R, G, and B stripes"
    );
  }

  #[test]
  fn gdi_text_surface_and_logfont_quality_select_the_bitmap_format() {
    assert_eq!(
      GdiTextSurface::Color.glyph_format(crate::wmf::WmfFontQuality::NonAntialiased.raw()),
      GdiGlyphFormat::Monochrome
    );
    assert_eq!(
      GdiTextSurface::Color.glyph_format(crate::wmf::WmfFontQuality::Antialiased.raw()),
      GdiGlyphFormat::Grayscale
    );
    assert_eq!(
      GdiTextSurface::Color.glyph_format(crate::wmf::WmfFontQuality::ClearType.raw()),
      GdiGlyphFormat::Lcd
    );
    assert_eq!(
      GdiTextSurface::Monochrome.glyph_format(crate::wmf::WmfFontQuality::ClearType.raw()),
      GdiGlyphFormat::Monochrome,
      "an indexed destination overrides LOGFONT smoothing quality"
    );
  }

  fn rectangular_dropout_test_path(left: f32, top: f32, right: f32, bottom: f32) -> TinySkiaPath {
    let mut builder = TinySkiaPathBuilder::new();
    builder.move_to(left, top);
    builder.line_to(right, top);
    builder.line_to(right, bottom);
    builder.line_to(left, bottom);
    builder.close();
    builder.finish().unwrap()
  }

  fn rasterize_dropout_test_path(path: &TinySkiaPath, width: usize, height: usize) -> Vec<u8> {
    let mut mask = TinySkiaMask::new(width as u32, height as u32).unwrap();
    mask.fill_path(
      path,
      TinySkiaFillRule::Winding,
      false,
      TinySkiaTransform::identity(),
    );
    let mut coverage = mask.take();
    apply_gdi_smart_dropout_control(path, &mut coverage, width, height);
    coverage
  }

  #[test]
  fn smart_dropout_recovers_a_vertical_stem_in_both_directions() {
    let path = rectangular_dropout_test_path(0.75, 0.0, 1.25, 4.0);

    assert_eq!(
      rasterize_dropout_test_path(&path, 2, 4),
      [0, 0, u8::MAX, 0, u8::MAX, 0, 0, 0],
      "rule 6 excludes the two terminal stubs but preserves the continuing stem"
    );
  }

  #[test]
  fn smart_dropout_second_pass_recovers_a_horizontal_stem() {
    let path = rectangular_dropout_test_path(0.0, 0.75, 4.0, 1.25);

    assert_eq!(
      rasterize_dropout_test_path(&path, 4, 2),
      [0, 0, 0, 0, 0, u8::MAX, u8::MAX, 0],
      "the perpendicular scan is required for horizontal drop-outs"
    );
  }

  #[test]
  fn smart_dropout_excludes_an_isolated_stub() {
    let path = rectangular_dropout_test_path(0.75, 1.0, 1.25, 2.0);

    assert_eq!(rasterize_dropout_test_path(&path, 2, 3), [0; 6]);
  }

  #[test]
  fn smart_dropout_does_not_duplicate_an_already_set_neighbor() {
    let path = rectangular_dropout_test_path(0.75, 0.0, 1.25, 4.0);
    let mut coverage = vec![0; 8];
    coverage[3] = u8::MAX;
    coverage[5] = u8::MAX;

    apply_gdi_smart_dropout_control(&path, &mut coverage, 2, 4);

    assert_eq!(coverage, [0, 0, 0, u8::MAX, 0, u8::MAX, 0, 0]);
  }

  #[test]
  fn ggo_bitmap_rows_are_msb_first_and_dword_aligned() {
    let (stride, bits) = pack_gdi_monochrome_mask(
      &[
        255, 0, 0, 0, 0, 0, 0, 0, 255, // row 0: bits 0 and 8
        0, 255, 0, 0, 0, 0, 0, 255, 0, // row 1: bits 1 and 7
      ],
      9,
      2,
    )
    .unwrap();

    assert_eq!(stride, 4);
    assert_eq!(bits, [0x80, 0x80, 0, 0, 0x41, 0, 0, 0]);
  }

  #[test]
  fn transparent_color_replay_uses_the_monochrome_text_alpha() {
    let rgba = straight_rgba_from_black_white_with_mask(
      &[0, 0, 0],
      &[127, 127, 127],
      &[0, 0, 0],
      &[0, 0, 0],
    )
    .unwrap();

    assert_eq!(rgba, [0, 0, 0, 255]);
  }

  #[test]
  fn binary_coverage_surface_keeps_black_matte_rgb_and_any_stripe() {
    let rgba = straight_rgba_with_binary_coverage(
      &[0, 0, 0, 0, 0, 0, 255, 255, 255, 32, 0, 0],
      &[13, 8, 4, 255, 255, 255, 255, 255, 255, 255, 127, 255],
      &[0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0],
      &[255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255],
    )
    .unwrap();

    assert_eq!(
      rgba,
      [
        0, 0, 0, 255, // ClearType edge: black-matte RGB with binary coverage
        0, 0, 0, 0, // untouched background
        255, 255, 255, 255, // an opaque white source pixel
        32, 0, 0, 255, // one covered stripe keeps the black-matte RGB sample
      ]
    );
  }

  #[test]
  fn gdi_subpixel_blend_matches_the_dib_driver_integer_formula() {
    assert_eq!(gdi_subpixel_blend(255, 0, 128), 127);
    assert_eq!(gdi_subpixel_blend(17, 201, 0), 17);
    assert_eq!(gdi_subpixel_blend(17, 201, 255), 201);
  }

  #[test]
  fn crop_raster_pixels_matches_the_valid_source_rectangle() {
    let image = RasterPixels {
      width: 3,
      height: 2,
      rgb: vec![
        255, 0, 0, 0, 255, 0, 0, 0, 255, // first row
        255, 255, 0, 0, 255, 255, 255, 0, 255, // second row
      ],
    };
    let cropped = crop_raster_pixels(&image, (1, 0, 2, 2)).unwrap();
    assert_eq!(cropped.width, 2);
    assert_eq!(cropped.height, 2);
    assert_eq!(
      cropped.rgb,
      [0, 255, 0, 0, 0, 255, 0, 255, 255, 255, 0, 255,]
    );
    assert!(crop_raster_pixels(&image, (-1, 0, 2, 2)).is_none());
    assert!(crop_raster_pixels(&image, (2, 0, 2, 2)).is_none());
  }

  #[test]
  fn bilinear_stretch_samples_pixel_centers_without_blurring_the_outer_edge() {
    let image = RasterPixels {
      width: 2,
      height: 1,
      rgb: vec![0, 0, 0, 255, 255, 255],
    };

    assert_eq!(bilinear_raster_color(&image, 0, 0, 4, 1).r, 0);
    assert_eq!(bilinear_raster_color(&image, 1, 0, 4, 1).r, 64);
    assert_eq!(bilinear_raster_color(&image, 2, 0, 4, 1).r, 191);
    assert_eq!(bilinear_raster_color(&image, 3, 0, 4, 1).r, 255);
  }

  #[test]
  fn nearest_stretch_samples_destination_pixel_centers() {
    assert_eq!(
      (0..5)
        .map(|destination| nearest_raster_index(destination, 5, 2))
        .collect::<Vec<_>>(),
      [0, 0, 1, 1, 1]
    );
  }

  #[test]
  fn two_color_palette_rasters_stay_discrete_during_stretch() {
    let two_color = RasterPixels {
      width: 2,
      height: 1,
      rgb: vec![255, 255, 255, 0, 236, 236],
    };
    assert!(is_discrete_two_color_raster(&two_color));
    let mut three_color = two_color;
    three_color.width = 3;
    three_color.rgb.extend_from_slice(&[1, 2, 3]);
    assert!(!is_discrete_two_color_raster(&three_color));
  }

  #[test]
  fn non_metafile_returns_none() {
    assert!(
      decode_metafile_as_raster(b"not a metafile", None)
        .unwrap()
        .is_none()
    );
  }

  #[test]
  fn wmf_symbol_charset_decodes_glyph_bytes_as_private_use_characters() {
    assert_eq!(
      decode_wmf_text(b"wjlq\0", crate::wmf::WmfCharacterSet::Symbol.raw()),
      "\u{F077}\u{F06A}\u{F06C}\u{F071}"
    );
    assert_eq!(
      decode_wmf_text(b"ABC\0", crate::wmf::WmfCharacterSet::Ansi.raw()),
      "ABC"
    );
  }

  #[test]
  fn wmf_symbol_face_uses_symbol_charset_even_with_default_declared() {
    let mut face_name = [0; 32];
    face_name[..6].copy_from_slice(b"Symbol");
    let font = crate::wmf::WmfFontObject {
      height: -12,
      width: 0,
      escapement: 0,
      orientation: 0,
      weight: 400,
      italic: 0,
      underline: 0,
      strike_out: 0,
      char_set: crate::wmf::WmfCharacterSet::Default.raw(),
      out_precision: 0,
      clip_precision: 0,
      quality: 0,
      pitch_and_family: 0,
      face_name,
      face_name_bytes: 6,
    };
    assert_eq!(
      wmf_text_font(&font).char_set,
      crate::wmf::WmfCharacterSet::Symbol.raw()
    );
  }

  #[test]
  fn wmf_face_name_uses_declared_charset_with_ansi_fallback() {
    let mut face_name = [0; 32];
    face_name[..4].copy_from_slice(&[0xCB, 0xCE, 0xCC, 0xE5]);
    let mut font = crate::wmf::WmfFontObject {
      height: -12,
      width: 0,
      escapement: 0,
      orientation: 0,
      weight: 400,
      italic: 0,
      underline: 0,
      strike_out: 0,
      char_set: crate::wmf::WmfCharacterSet::Gb2312.raw(),
      out_precision: 0,
      clip_precision: 0,
      quality: 0,
      pitch_and_family: 0,
      face_name,
      face_name_bytes: 4,
    };
    assert_eq!(wmf_text_font(&font).family.as_deref(), Some("宋体"));

    font.face_name = [0; 32];
    font.face_name[..5].copy_from_slice(b"Arial");
    font.face_name_bytes = 5;
    font.char_set = 0xFE;
    assert_eq!(wmf_text_font(&font).family.as_deref(), Some("Arial"));
  }

  #[test]
  fn wmf_update_cp_uses_move_to_and_dx_for_consecutive_text_origins() {
    let records = vec![
      WmfRecordData::SetWindowExt(WmfPointRecord { x: 100, y: 100 })
        .to_record()
        .unwrap(),
      WmfRecordData::SetTextAlign(WmfU16Record {
        value: (WmfTextAlignmentModeFlags::UPDATE_CP | WmfTextAlignmentModeFlags::BASELINE).bits(),
        reserved: Vec::new(),
      })
      .to_record()
      .unwrap(),
      WmfRecordData::MoveTo(WmfPointRecord { x: 10, y: 20 })
        .to_record()
        .unwrap(),
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 99,
        x: 99,
        string_length: 2,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"AB".to_vec(),
        string_padding: Vec::new(),
        dx: vec![7, 5],
        trailing_data: Vec::new(),
      })
      .to_record()
      .unwrap(),
      WmfRecordData::ExtTextOut(WmfExtTextOutRecord {
        y: 99,
        x: 99,
        string_length: 1,
        options: WmfExtTextOutOptions::empty(),
        rectangle: None,
        string: b"C".to_vec(),
        string_padding: vec![0],
        dx: vec![3],
        trailing_data: Vec::new(),
      })
      .to_record()
      .unwrap(),
      WmfRecordData::Eof(crate::wmf::WmfEofRecord::default())
        .to_record()
        .unwrap(),
    ];
    let bytes = WmfMetafile {
      placeable_header: None,
      header: WmfHeader {
        metafile_type: WmfMetafileType::Memory.raw(),
        header_size_words: 9,
        version: WmfMetafileVersion::Version300.raw(),
        file_size_words: 0,
        number_of_objects: 0,
        max_record_words: 0,
        number_of_parameters: 0,
      },
      records,
      trailing_data: Vec::new(),
    }
    .to_bytes()
    .unwrap();

    let runs = extract_metafile_text_runs(&bytes, Some("image/x-wmf"));
    assert_eq!(runs.len(), 2);
    assert!((runs[0].x - 0.10).abs() < 0.000_1);
    assert!((runs[0].y - 0.20).abs() < 0.000_1);
    assert!((runs[0].width.unwrap() - 0.12).abs() < 0.000_1);
    assert!((runs[1].x - 0.22).abs() < 0.000_1);
    assert!((runs[1].y - 0.20).abs() < 0.000_1);
  }
}
