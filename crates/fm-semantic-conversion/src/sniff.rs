//! Deterministic format resolution.
//!
//! Dispatch must not depend on how a caller happened to label a file, because
//! a mislabelled container is exactly how a parser is pointed at bytes it was
//! not written for. The rules, applied in order:
//!
//! 1. **Magic bytes win for containers.** `%PDF-` is a PDF; a ZIP local file
//!    header is examined for the EPUB signature or OOXML marker parts and
//!    resolved accordingly. A declared media type never overrides this.
//! 2. **Then the declared media type**, for text-shaped formats.
//! 3. **Then the extension hint.**
//! 4. **Then the bytes.** Content with a NUL byte in its first 8 KiB and no
//!    recognized container signature is treated as binary and reported
//!    unsupported rather than decoded as mojibake.
//! 5. Otherwise the content is plain text.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use crate::formats::package;
use crate::model::{FormatKind, MediaType};

/// How many leading bytes are inspected when deciding whether content is
/// binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// Outcome of format resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedFormat {
    /// A baseline converter handles this content.
    Supported(FormatKind),
    /// No baseline converter handles it.
    Unsupported {
        /// Why the content is not handled.
        detail: String,
    },
    /// The content is a ZIP-shaped package that could not be read at all.
    Malformed {
        /// What was wrong.
        detail: String,
    },
}

fn format_for_media_type(media_type: &MediaType) -> Option<FormatKind> {
    match media_type.as_str() {
        "text/markdown" | "text/x-markdown" => Some(FormatKind::Markdown),
        "text/html" | "application/xhtml+xml" => Some(FormatKind::Html),
        "text/csv" | "text/tab-separated-values" => Some(FormatKind::Csv),
        "application/pdf" => Some(FormatKind::Pdf),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            Some(FormatKind::Docx)
        }
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
            Some(FormatKind::Pptx)
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            Some(FormatKind::Spreadsheet)
        }
        "text/plain" => Some(FormatKind::PlainText),
        other if other.starts_with("text/x-") => Some(FormatKind::SourceCode),
        _ => None,
    }
}

/// Extensions treated as source code, chunked by line range and symbol.
const CODE_EXTENSIONS: &[&str] = &[
    "c", "cc", "cjs", "cpp", "cs", "css", "cxx", "go", "h", "hpp", "hs", "java", "js", "json",
    "jsx", "kt", "lua", "m", "mjs", "php", "pl", "ps1", "py", "r", "rb", "rs", "scala", "scss",
    "sh", "sql", "swift", "toml", "ts", "tsx", "vue", "yaml", "yml", "zsh",
];

fn format_for_extension(extension: &str) -> Option<FormatKind> {
    match extension {
        "md" | "markdown" => Some(FormatKind::Markdown),
        "html" | "htm" | "xhtml" => Some(FormatKind::Html),
        "csv" | "tsv" => Some(FormatKind::Csv),
        "pdf" => Some(FormatKind::Pdf),
        "docx" => Some(FormatKind::Docx),
        "pptx" => Some(FormatKind::Pptx),
        "xlsx" | "xlsm" => Some(FormatKind::Spreadsheet),
        "txt" | "text" | "log" => Some(FormatKind::PlainText),
        other if CODE_EXTENSIONS.contains(&other) => Some(FormatKind::SourceCode),
        _ => None,
    }
}

/// Recognizes the OOXML flavour of a ZIP package by its marker part.
fn zip_format(bytes: &[u8]) -> ResolvedFormat {
    let mut archive = match zip::ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => archive,
        Err(error) => {
            return ResolvedFormat::Malformed {
                detail: format!("the ZIP container could not be read: {error}"),
            };
        }
    };
    let mut has_word = false;
    let mut has_presentation = false;
    let mut has_workbook = false;
    let mut epub_mimetype_entries = 0_u32;
    let mut epub_mimetype_is_stored = false;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let entry = match archive.by_index_raw(index) {
            Ok(entry) => entry,
            Err(error) => {
                return ResolvedFormat::Malformed {
                    detail: format!("ZIP entry {index} is unreadable: {error}"),
                };
            }
        };
        let name = entry.name().to_owned();
        if let Some(detail) = package::unsafe_entry_detail(
            &name,
            entry.enclosed_name().is_some(),
            entry.size(),
            entry.compressed_size(),
        ) {
            return ResolvedFormat::Malformed { detail };
        }
        if !names.insert(name.clone()) {
            return ResolvedFormat::Malformed {
                detail: format!("ZIP package contains duplicate entry '{name}'"),
            };
        }
        match name.as_str() {
            "word/document.xml" => has_word = true,
            "ppt/presentation.xml" => has_presentation = true,
            "xl/workbook.xml" | "xl/workbook.bin" => has_workbook = true,
            "mimetype" => {
                epub_mimetype_entries += 1;
                epub_mimetype_is_stored = entry.compression() == zip::CompressionMethod::Stored;
            }
            _ => {}
        }
    }
    if epub_mimetype_entries == 1 && epub_mimetype_is_stored {
        let mut mimetype = Vec::new();
        if archive
            .by_name("mimetype")
            .and_then(|mut entry| {
                entry
                    .by_ref()
                    .take(64)
                    .read_to_end(&mut mimetype)
                    .map_err(zip::result::ZipError::Io)
            })
            .is_ok()
            && mimetype == b"application/epub+zip"
        {
            return ResolvedFormat::Supported(FormatKind::Epub);
        }
    }
    if has_word {
        return ResolvedFormat::Supported(FormatKind::Docx);
    }
    if has_presentation {
        return ResolvedFormat::Supported(FormatKind::Pptx);
    }
    if has_workbook {
        return ResolvedFormat::Supported(FormatKind::Spreadsheet);
    }
    ResolvedFormat::Unsupported {
        detail: "the ZIP package is not a DOCX, PPTX or XLSX document".to_owned(),
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .take(BINARY_SNIFF_BYTES)
        .any(|byte| *byte == 0x00)
}

/// Resolves which baseline converter should handle `bytes`.
///
/// `declared` and `extension` are hints only; see the module documentation for
/// the precedence rules.
#[must_use]
pub fn resolve_format(
    bytes: &[u8],
    declared: Option<&MediaType>,
    extension: Option<&str>,
) -> ResolvedFormat {
    if bytes.starts_with(b"%PDF-") {
        return ResolvedFormat::Supported(FormatKind::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return zip_format(bytes);
    }
    if let Some(format) = declared.and_then(format_for_media_type) {
        // A declared container type that the magic bytes just contradicted is
        // not trusted: those formats are only reachable through their
        // signature.
        if !matches!(
            format,
            FormatKind::Pdf | FormatKind::Docx | FormatKind::Pptx | FormatKind::Spreadsheet
        ) {
            return ResolvedFormat::Supported(format);
        }
    }
    if let Some(format) = extension.and_then(format_for_extension)
        && !matches!(
            format,
            FormatKind::Pdf | FormatKind::Docx | FormatKind::Pptx | FormatKind::Spreadsheet
        )
    {
        return ResolvedFormat::Supported(format);
    }
    if declared.is_some_and(|media| !media.is_text() && format_for_media_type(media).is_none())
        && looks_binary(bytes)
    {
        return ResolvedFormat::Unsupported {
            detail: format!(
                "no baseline converter handles {}",
                declared.map_or("this media type", MediaType::as_str)
            ),
        };
    }
    if looks_binary(bytes) {
        return ResolvedFormat::Unsupported {
            detail: "the content is binary and has no recognized document signature".to_owned(),
        };
    }
    ResolvedFormat::Supported(FormatKind::PlainText)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut buffer);
            for (name, data) in entries {
                archive
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start entry");
                archive.write_all(data).expect("write entry");
            }
            archive.finish().expect("finish archive");
        }
        buffer.into_inner()
    }

    #[test]
    fn magic_bytes_beat_a_misleading_declared_media_type() {
        let declared = MediaType::parse("text/plain");
        assert_eq!(
            resolve_format(b"%PDF-1.7\n...", Some(&declared), Some("txt")),
            ResolvedFormat::Supported(FormatKind::Pdf)
        );
        let docx = package(&[("word/document.xml", b"<w:document/>")]);
        assert_eq!(
            resolve_format(&docx, Some(&declared), Some("txt")),
            ResolvedFormat::Supported(FormatKind::Docx)
        );
    }

    #[test]
    fn ooxml_flavours_are_distinguished_by_their_marker_parts() {
        let pptx = package(&[("ppt/presentation.xml", b"<p:presentation/>")]);
        let xlsx = package(&[("xl/workbook.xml", b"<workbook/>")]);
        assert_eq!(
            resolve_format(&pptx, None, None),
            ResolvedFormat::Supported(FormatKind::Pptx)
        );
        assert_eq!(
            resolve_format(&xlsx, None, None),
            ResolvedFormat::Supported(FormatKind::Spreadsheet)
        );
    }

    #[test]
    fn an_unrecognized_zip_is_unsupported_and_a_broken_one_is_malformed() {
        let other = package(&[("readme.txt", b"hello")]);
        assert!(matches!(
            resolve_format(&other, None, None),
            ResolvedFormat::Unsupported { .. }
        ));
        assert!(matches!(
            resolve_format(b"PK\x03\x04garbage", None, None),
            ResolvedFormat::Malformed { .. }
        ));
    }

    #[test]
    fn a_declared_container_type_without_its_signature_is_not_trusted() {
        let declared = MediaType::parse("application/pdf");
        assert_eq!(
            resolve_format(b"just text", Some(&declared), None),
            ResolvedFormat::Supported(FormatKind::PlainText)
        );
    }

    #[test]
    fn text_formats_come_from_the_media_type_then_the_extension() {
        let markdown = MediaType::parse("text/markdown");
        assert_eq!(
            resolve_format(b"# Title", Some(&markdown), Some("rs")),
            ResolvedFormat::Supported(FormatKind::Markdown)
        );
        assert_eq!(
            resolve_format(b"fn main() {}", None, Some("rs")),
            ResolvedFormat::Supported(FormatKind::SourceCode)
        );
        assert_eq!(
            resolve_format(b"plain", None, None),
            ResolvedFormat::Supported(FormatKind::PlainText)
        );
    }

    #[test]
    fn binary_content_without_a_signature_is_unsupported() {
        assert!(matches!(
            resolve_format(&[0x00, 0x01, 0x02, 0x03], None, None),
            ResolvedFormat::Unsupported { .. }
        ));
    }

    #[test]
    fn resolution_is_deterministic_for_the_same_input() {
        let docx = package(&[
            ("xl/workbook.xml", b"<workbook/>"),
            ("word/document.xml", b"<w:document/>"),
        ]);
        for _ in 0..8 {
            assert_eq!(
                resolve_format(&docx, None, None),
                ResolvedFormat::Supported(FormatKind::Docx)
            );
        }
    }
}
