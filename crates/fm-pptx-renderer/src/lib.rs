//! Narrow adapter around the vendored OOXML renderer.
//!
//! Keeping the upstream API behind this crate limits replacement to one module when
//! `ooxmlsdk-pdf` publishes a stable release.

use std::io::Cursor;

use ooxmlsdk_pdf::{PdfOptions, convert_pptx, convert_pptx_first_page};
use thiserror::Error;

/// Maximum retained PDF output from one presentation conversion.
pub const MAX_RENDERED_PDF_BYTES: usize = 64 * 1024 * 1024;
/// Maximum first-page PDF returned inline by the preview-open contract.
pub const MAX_FIRST_PAGE_PDF_BYTES: usize = 8 * 1024 * 1024;

/// Failure converting a PPTX package to a bounded PDF.
#[derive(Debug, Error)]
pub enum PptxRenderError {
    /// The upstream renderer rejected the presentation.
    #[error("could not render the PowerPoint presentation: {0}")]
    Conversion(String),
    /// The rendered PDF exceeds the preview-session memory budget.
    #[error("rendered PowerPoint preview exceeds the PDF output limit of 64 MiB")]
    OutputTooLarge,
    /// The immediate first-page response exceeds its transport budget.
    #[error("first-page PowerPoint preview exceeds the inline response limit of 8 MiB")]
    FirstPageTooLarge,
}

/// Converts one in-memory PPTX package into bounded PDF bytes.
pub fn render_pptx_to_pdf(bytes: &[u8]) -> Result<Vec<u8>, PptxRenderError> {
    render_bounded(|| convert_pptx(Cursor::new(bytes), PdfOptions::default()))
}

/// Converts the first slide into a bounded PDF for immediate display.
pub fn render_pptx_first_page_to_pdf(bytes: &[u8]) -> Result<Vec<u8>, PptxRenderError> {
    let pdf =
        render_bounded(|| convert_pptx_first_page(Cursor::new(bytes), PdfOptions::default()))?;
    if pdf.len() > MAX_FIRST_PAGE_PDF_BYTES {
        return Err(PptxRenderError::FirstPageTooLarge);
    }
    Ok(pdf)
}

fn render_bounded<E: std::fmt::Display>(
    render: impl FnOnce() -> Result<Vec<u8>, E>,
) -> Result<Vec<u8>, PptxRenderError> {
    let pdf = render().map_err(|error| PptxRenderError::Conversion(error.to_string()))?;
    if pdf.len() > MAX_RENDERED_PDF_BYTES {
        return Err(PptxRenderError::OutputTooLarge);
    }
    Ok(pdf)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn converts_a_presentation_slide_to_pdf() {
        let source = package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
            ),
            (
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="10000000" cy="1000000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3200"/><a:t>Hello PPTX</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
        ]);

        let pdf = render_pptx_to_pdf(&source).expect("presentation must render");

        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn substitutes_an_unavailable_office_font_with_a_shapeable_system_font() {
        let source = package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
            ),
            (
                "ppt/presentation.xml",
                br#"<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                br#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="914400" y="914400"/><a:ext cx="10000000" cy="1000000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3200" typeface="Aptos Display"/><a:t>Visible title</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
            ),
        ]);

        let output = ooxmlsdk_pdf::convert_pptx_with_font_audit(
            std::io::Cursor::new(source),
            ooxmlsdk_pdf::PdfOptions::default(),
        )
        .expect("presentation must render");

        assert_eq!(output.audit.painted_text_portion_count, 1);
        assert!(output.audit.glyph_run_count > 0, "{:#?}", output.audit);
        assert!(output.audit.glyph_count > 0, "{:#?}", output.audit);
        assert!(output.audit.issues.is_empty(), "{:#?}", output.audit);
    }

    fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut bytes);
            for (name, data) in entries {
                archive
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start fixture entry");
                archive.write_all(data).expect("write fixture entry");
            }
            archive.finish().expect("finish fixture");
        }
        bytes.into_inner()
    }
}
