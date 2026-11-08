//! Plain-text extraction from document formats for content search: Office Open XML
//! (docx/xlsx/pptx), OpenDocument (odt/ods/odp) and EPUB, all of which are ZIP packages of XML.
//!
//! Pure and bounded: a package is only read as far as [`MAX_TOTAL_XML_BYTES`] of decompressed XML,
//! so a hostile archive cannot exhaust memory.

use std::io::{Cursor, Read};

use zip::ZipArchive;

/// Decompressed XML read from one package before the rest is ignored.
const MAX_TOTAL_XML_BYTES: u64 = 32 * 1024 * 1024;

/// Whether `bytes` start like a ZIP package.
#[must_use]
pub(crate) fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Which text-bearing parts of a package to read, in reading order.
fn text_parts(names: &[String], mimetype: Option<&str>) -> Vec<String> {
    let has = |name: &str| names.iter().any(|candidate| candidate == name);
    let numbered = |prefix: &str, suffix: &str| {
        let mut parts: Vec<(u32, String)> = names
            .iter()
            .filter_map(|name| {
                let number = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
                Some((number.parse().unwrap_or(u32::MAX), name.clone()))
            })
            .collect();
        parts.sort();
        parts.into_iter().map(|(_, name)| name).collect::<Vec<_>>()
    };
    let by_prefix = |prefix: &str| {
        let mut parts: Vec<String> = names
            .iter()
            .filter(|name| name.starts_with(prefix) && name.ends_with(".xml"))
            .cloned()
            .collect();
        parts.sort();
        parts
    };
    if has("word/document.xml") {
        let mut parts = vec!["word/document.xml".to_owned()];
        for extra in [
            "word/footnotes.xml",
            "word/endnotes.xml",
            "word/comments.xml",
        ] {
            if has(extra) {
                parts.push(extra.to_owned());
            }
        }
        parts.extend(by_prefix("word/header"));
        parts.extend(by_prefix("word/footer"));
        return parts;
    }
    if has("ppt/presentation.xml") {
        let mut parts = numbered("ppt/slides/slide", ".xml");
        parts.extend(numbered("ppt/notesSlides/notesSlide", ".xml"));
        return parts;
    }
    if has("xl/workbook.xml") {
        let mut parts = Vec::new();
        if has("xl/sharedStrings.xml") {
            parts.push("xl/sharedStrings.xml".to_owned());
        }
        parts.extend(numbered("xl/worksheets/sheet", ".xml"));
        return parts;
    }
    if mimetype.is_some_and(|value| value.starts_with("application/vnd.oasis.opendocument."))
        && has("content.xml")
    {
        return vec!["content.xml".to_owned()];
    }
    if mimetype == Some("application/epub+zip") {
        let mut parts: Vec<String> = names
            .iter()
            .filter(|name| {
                let lower = name.to_lowercase();
                lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
            })
            .cloned()
            .collect();
        parts.sort();
        return parts;
    }
    Vec::new()
}

/// Extracts the text of a document package, one paragraph or row per line. `None` when the bytes
/// are not a recognised document (e.g. an ordinary ZIP) or hold no text.
#[must_use]
pub(crate) fn document_text(bytes: &[u8]) -> Option<String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    let mimetype = archive.by_name("mimetype").ok().and_then(|mut entry| {
        let mut text = String::new();
        entry.by_ref().take(256).read_to_string(&mut text).ok()?;
        Some(text.trim().to_owned())
    });
    let parts = text_parts(&names, mimetype.as_deref());
    if parts.is_empty() {
        return None;
    }
    let mut budget = MAX_TOTAL_XML_BYTES;
    let mut text = String::new();
    for part in parts {
        if budget == 0 {
            break;
        }
        let Ok(entry) = archive.by_name(&part) else {
            continue;
        };
        let mut raw = Vec::new();
        if entry.take(budget).read_to_end(&mut raw).is_err() {
            continue;
        }
        budget = budget.saturating_sub(raw.len() as u64);
        xml_to_text(&String::from_utf8_lossy(&raw), &mut text);
        if !text.ends_with('\n') {
            text.push('\n');
        }
    }
    (!text.trim().is_empty()).then_some(text)
}

/// Local name of a tag (`w:p` -> `p`, `/text:p` -> `p`), lower-cased.
fn local_name(tag: &str) -> String {
    let name = tag
        .trim_start_matches('/')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("");
    name.rsplit(':').next().unwrap_or("").to_ascii_lowercase()
}

/// Appends the text content of an XML/HTML fragment to `out`: tags removed, entities decoded,
/// block-level tags (paragraphs, rows, breaks, list items, headings) turned into line breaks and
/// tabs into spaces. `script` and `style` content is dropped.
pub(crate) fn xml_to_text(xml: &str, out: &mut String) {
    let mut rest = xml;
    let mut skipping: Option<&'static str> = None;
    while let Some(open) = rest.find('<') {
        let (before, after) = rest.split_at(open);
        if skipping.is_none() {
            decode_entities(before, out);
        }
        let Some(close) = after.find('>') else {
            return;
        };
        let tag = &after[1..close];
        rest = &after[close + 1..];
        if tag.starts_with('!') || tag.starts_with('?') {
            continue;
        }
        let name = local_name(tag);
        let closing = tag.starts_with('/');
        if let Some(skipped) = skipping {
            if closing && name == skipped {
                skipping = None;
            }
            continue;
        }
        if !closing && (name == "script" || name == "style") && !tag.ends_with('/') {
            skipping = Some(if name == "script" { "script" } else { "style" });
            continue;
        }
        let block = matches!(
            name.as_str(),
            "p" | "tr"
                | "row"
                | "li"
                | "div"
                | "h"
                | "title"
                | "si"
                | "section"
                | "table"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
        );
        match name.as_str() {
            "tab" => out.push(' '),
            "td" | "th" | "c" if closing => out.push(' '),
            "br" | "line-break" | "cr" => out.push('\n'),
            _ if block && closing && !out.ends_with('\n') => out.push('\n'),
            _ => {}
        }
    }
    if skipping.is_none() {
        decode_entities(rest, out);
    }
}

fn decode_entities(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let decoded = after.find(';').filter(|end| *end <= 10).and_then(|end| {
            let entity = &after[..end];
            let character = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => entity
                    .strip_prefix('#')
                    .and_then(|number| match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => number.parse().ok(),
                    })
                    .and_then(char::from_u32),
            }?;
            Some((character, end))
        });
        match decoded {
            Some((character, end)) => {
                out.push(character);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in parts {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn strips_tags_decodes_entities_and_breaks_paragraphs() {
        let mut out = String::new();
        xml_to_text(
            "<w:p><w:r><w:t>Staal &amp; ijzer &#233;</w:t></w:r></w:p><w:p><w:t>tweede</w:t><w:tab/><w:t>x</w:t></w:p>",
            &mut out,
        );
        assert_eq!(out, "Staal & ijzer é\ntweede x\n");
    }

    #[test]
    fn drops_script_and_style_content() {
        let mut out = String::new();
        xml_to_text(
            "<html><style>.a{color:red}</style><p>visible</p><script>hidden()</script></html>",
            &mut out,
        );
        assert_eq!(out.trim(), "visible");
    }

    #[test]
    fn reads_docx_pptx_xlsx_odt_and_epub_text() {
        let docx = package(&[(
            "word/document.xml",
            "<w:document><w:p><w:t>docx Staal</w:t></w:p></w:document>",
        )]);
        assert!(document_text(&docx).unwrap().contains("docx Staal"));

        let pptx = package(&[
            ("ppt/presentation.xml", "<p/>"),
            ("ppt/slides/slide2.xml", "<a:p><a:t>second</a:t></a:p>"),
            ("ppt/slides/slide10.xml", "<a:p><a:t>tenth</a:t></a:p>"),
            ("ppt/slides/slide1.xml", "<a:p><a:t>first Staal</a:t></a:p>"),
        ]);
        let text = document_text(&pptx).unwrap();
        assert!(text.find("first").unwrap() < text.find("second").unwrap());
        assert!(text.find("second").unwrap() < text.find("tenth").unwrap());

        let xlsx = package(&[
            ("xl/workbook.xml", "<workbook/>"),
            (
                "xl/sharedStrings.xml",
                "<sst><si><t>cel Staal</t></si></sst>",
            ),
        ]);
        assert!(document_text(&xlsx).unwrap().contains("cel Staal"));

        let odt = package(&[
            ("mimetype", "application/vnd.oasis.opendocument.text"),
            (
                "content.xml",
                "<office:text><text:p>odt Staal</text:p></office:text>",
            ),
        ]);
        assert!(document_text(&odt).unwrap().contains("odt Staal"));

        let epub = package(&[
            ("mimetype", "application/epub+zip"),
            (
                "OEBPS/ch1.xhtml",
                "<html><body><p>epub Staal</p></body></html>",
            ),
        ]);
        assert!(document_text(&epub).unwrap().contains("epub Staal"));
    }

    #[test]
    fn an_ordinary_zip_is_not_a_document() {
        let plain = package(&[("notes.txt", "Staal")]);
        assert!(looks_like_zip(&plain));
        assert_eq!(document_text(&plain), None);
        assert_eq!(document_text(b"not a zip"), None);
    }
}
