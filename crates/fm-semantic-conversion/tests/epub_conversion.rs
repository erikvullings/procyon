//! Public-interface regressions for bounded EPUB semantic conversion.

use std::io::{Cursor, Write};

use fm_semantic_conversion::{
    BASELINE_CONVERTER_VERSION, BaselineConverter, BudgetKind, CancellationFlag, ChunkProvenance,
    Chunker, Completeness, ConversionBudgets, ConversionContext, ConversionOutcome,
    DocumentConverter, DocumentMetadata, FormatKind, Provenance, ResolvedFormat, SourceContent,
    TopLevelBoundary, resolve_format,
};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

fn package(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut buffer);
        for (name, bytes, method) in entries {
            archive
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(*method),
                )
                .expect("start package entry");
            archive.write_all(bytes).expect("write package entry");
        }

        archive.finish().expect("finish package");
    }
    buffer.into_inner()
}

fn epub(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut package_entries = vec![(
        "mimetype",
        b"application/epub+zip".as_slice(),
        CompressionMethod::Stored,
    )];
    package_entries.extend(
        entries
            .iter()
            .map(|(name, bytes)| (*name, *bytes, CompressionMethod::Deflated)),
    );
    package(&package_entries)
}

const CONTAINER: &[u8] = br#"<?xml version="1.0"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles>
</container>"#;

fn representative_epub() -> Vec<u8> {
    package(&[
        (
            "mimetype",
            b"application/epub+zip",
            CompressionMethod::Stored,
        ),
        (
            "META-INF/container.xml",
            br#"<?xml version="1.0"?>
    <c:container xmlns:c="urn:oasis:names:tc:opendocument:xmlns:container">
      <c:rootfiles><c:rootfile c:full-path="OPS/./package.opf"/></c:rootfiles>
    </c:container>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OPS/package.opf",
            br#"<?xml version="1.0"?>
    <opf:package xmlns:opf="http://www.idpf.org/2007/opf">
      <opf:manifest>
        <opf:item id="first" href="./text/first.xhtml#start" media-type="application/xhtml+xml"/>
        <opf:item id="second" href="text/second.html" media-type="text/html"/>
        <opf:item id="image" href="media/cover.png" media-type="image/png"/>
        <opf:item id="svg" href="media/diagram.svg" media-type="image/svg+xml"/>
        <opf:item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
        <opf:item id="css" href="book.css" media-type="text/css"/>
        <opf:item id="font" href="book.woff2" media-type="font/woff2"/>
        <opf:item id="audio" href="book.mp3" media-type="audio/mpeg"/>
        <opf:item id="video" href="book.mp4" media-type="video/mp4"/>
        <opf:item id="hidden" href="hidden.xhtml" media-type="application/xhtml+xml"/>
      </opf:manifest>
      <opf:spine>
        <opf:itemref idref="second"/>
        <opf:itemref idref="image"/>
        <opf:itemref idref="svg"/>
        <opf:itemref idref="nav"/>
        <opf:itemref idref="css"/>
        <opf:itemref idref="font"/>
        <opf:itemref idref="audio"/>
        <opf:itemref idref="video"/>
        <opf:itemref idref="first"/>
      </opf:spine>
    </opf:package>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OPS/text/first.xhtml",
            br#"<html>
    <body>
    <h1>First chapter</h1>
    <p>First body.</p>
    <script>FIRST_SCRIPT_MUST_NOT_APPEAR</script>
    <style>FIRST_STYLE_MUST_NOT_APPEAR</style>
    <svg><text>FIRST_SVG_TEXT_MUST_NOT_APPEAR</text></svg>
    </body>
    </html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OPS/text/second.html",
            br#"<x:html xmlns:x="http://www.w3.org/1999/xhtml">
    <x:body>
    <x:h1>Second chapter</x:h1>
    <x:p>Second body.</x:p>
    <x:ul><x:li>Second list item.</x:li></x:ul>
    <x:blockquote>Second quotation.</x:blockquote>
    <x:table><x:tr><x:td>A</x:td><x:td>B</x:td></x:tr></x:table>
    </x:body>
    </x:html>"#,
            CompressionMethod::Deflated,
        ),
        (
            "OPS/media/cover.png",
            b"IMAGE_BYTES_MUST_NOT_APPEAR",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/media/diagram.svg",
            b"<svg><text>MANIFEST_SVG_MUST_NOT_APPEAR</text></svg>",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/nav.xhtml",
            b"<html><body>NAVIGATION_MUST_NOT_APPEAR</body></html>",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/hidden.xhtml",
            b"<html><body>NON_SPINE_MUST_NOT_APPEAR</body></html>",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/book.css",
            b"STYLE_RESOURCE_MUST_NOT_APPEAR",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/book.woff2",
            b"FONT_BYTES_MUST_NOT_APPEAR",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/book.mp3",
            b"AUDIO_BYTES_MUST_NOT_APPEAR",
            CompressionMethod::Deflated,
        ),
        (
            "OPS/book.mp4",
            b"VIDEO_BYTES_MUST_NOT_APPEAR",
            CompressionMethod::Deflated,
        ),
    ])
}

fn convert(bytes: &[u8]) -> fm_semantic_conversion::ConversionOutcome {
    convert_with_context(bytes, &ConversionContext::new())
}

fn convert_with_context(
    bytes: &[u8],
    context: &ConversionContext,
) -> fm_semantic_conversion::ConversionOutcome {
    BaselineConverter::new()
        .convert(
            SourceContent::Bytes(bytes),
            &DocumentMetadata::unknown().with_extension("epub"),
            context,
        )
        .expect("conversion")
}

#[test]
fn epub_recognition_requires_the_exact_uncompressed_root_mimetype() {
    let valid = package(&[
        (
            "mimetype",
            b"application/epub+zip",
            CompressionMethod::Stored,
        ),
        (
            "META-INF/container.xml",
            b"<container/>",
            CompressionMethod::Deflated,
        ),
    ]);
    assert_eq!(
        resolve_format(&valid, None, Some("bin")),
        ResolvedFormat::Supported(FormatKind::Epub)
    );

    let compressed = package(&[(
        "mimetype",
        b"application/epub+zip",
        CompressionMethod::Deflated,
    )]);
    assert!(!matches!(
        resolve_format(&compressed, None, Some("epub")),
        ResolvedFormat::Supported(FormatKind::Epub)
    ));

    let wrong_value = package(&[(
        "mimetype",
        b"application/epub+zip\n",
        CompressionMethod::Stored,
    )]);
    assert!(!matches!(
        resolve_format(&wrong_value, None, Some("epub")),
        ResolvedFormat::Supported(FormatKind::Epub)
    ));

    let extension_only = DocumentMetadata::unknown().with_extension("epub");
    assert!(!matches!(
        resolve_format(
            b"not an EPUB package",
            extension_only.media_type(),
            extension_only.extension()
        ),
        ResolvedFormat::Supported(FormatKind::Epub)
    ));

    let unsafe_package = epub(&[
        ("../escape.xhtml", b"<p>escape</p>"),
        ("META-INF/container.xml", CONTAINER),
    ]);
    assert!(matches!(
        resolve_format(&unsafe_package, None, Some("epub")),
        ResolvedFormat::Malformed { .. }
    ));
}

#[test]
fn conversion_follows_the_namespace_neutral_spine_and_only_extracts_readable_html() {
    assert_eq!(BASELINE_CONVERTER_VERSION.to_string(), "baseline/2");

    let outcome = convert(&representative_epub());
    let document = outcome.document().expect("converted EPUB");
    assert_eq!(document.format(), FormatKind::Epub);
    assert_eq!(document.converter(), BASELINE_CONVERTER_VERSION);

    let text = document
        .units()
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        text,
        [
            "Second chapter",
            "Second body.",
            "Second list item.",
            "Second quotation.",
            "A | B",
            "First chapter",
            "First body.",
        ]
    );
    assert_eq!(document.units()[1].section_path, ["Second chapter"]);
    assert_eq!(document.units()[6].section_path, ["First chapter"]);
    assert!(matches!(
        document.units()[0].provenance,
        Provenance::EpubText {
            spine_index: 0,
            start_line: 3,
            end_line: 3,
        }
    ));
    assert!(matches!(
        document.units()[6].provenance,
        Provenance::EpubText { spine_index: 8, .. }
    ));
    assert_eq!(document.units()[0].boundary, TopLevelBoundary::EpubSpine(0));
    assert_eq!(document.units()[6].boundary, TopLevelBoundary::EpubSpine(8));
    assert!(
        text.iter().all(|value| !value.contains("MUST_NOT_APPEAR")),
        "ignored package resources leaked into semantic text"
    );

    let first_chunks = Chunker::default().chunk(document);
    let second_chunks = Chunker::default().chunk(document);
    assert_eq!(first_chunks, second_chunks);
    assert!(first_chunks.iter().all(|chunk| !matches!(
        chunk.provenance,
        ChunkProvenance::Exact(Provenance::PdfBlock { .. })
    )));
}

#[test]
fn malformed_missing_unsafe_and_encrypted_epubs_have_typed_outcomes() {
    let malformed_container = epub(&[(
        "META-INF/container.xml",
        b"<container><rootfiles><rootfile full-path=\"OPS/package.opf\"></rootfiles>",
    )]);
    assert!(matches!(
        convert(&malformed_container),
        ConversionOutcome::Malformed { .. }
    ));

    let unsafe_rootfile = epub(&[(
        "META-INF/container.xml",
        b"<container><rootfiles><rootfile full-path=\"/OPS/package.opf\"/></rootfiles></container>",
    )]);
    assert!(matches!(
        convert(&unsafe_rootfile),
        ConversionOutcome::Malformed { .. }
    ));

    let unsafe_manifest_path = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"chapter\" href=\"../escape.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>",
        ),
    ]);
    assert!(matches!(
        convert(&unsafe_manifest_path),
        ConversionOutcome::Malformed { .. }
    ));

    let missing_spine_resource = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"chapter\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>",
        ),
    ]);
    assert!(matches!(
        convert(&missing_spine_resource),
        ConversionOutcome::Malformed { .. }
    ));

    let no_readable_spine = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"cover\" href=\"cover.png\" media-type=\"image/png\"/></manifest><spine><itemref idref=\"cover\"/></spine></package>",
        ),
        ("OPS/cover.png", b"not semantic text"),
    ]);
    assert!(matches!(
        convert(&no_readable_spine),
        ConversionOutcome::Malformed { .. }
    ));

    let encrypted = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "META-INF/encryption.xml",
            b"<encryption><EncryptedData><CipherData><CipherReference URI=\"OPS/chapter.xhtml\"/></CipherData></EncryptedData></encryption>",
        ),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"chapter\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>",
        ),
        ("OPS/chapter.xhtml", b"<html><body><p>secret</p></body></html>"),
    ]);
    assert!(matches!(
        convert(&encrypted),
        ConversionOutcome::Encrypted { .. }
    ));
}

#[test]
fn cancellation_and_source_package_item_and_output_budgets_are_visible() {
    let bytes = representative_epub();

    let flag = CancellationFlag::new();
    flag.cancel();
    let cancelled = convert_with_context(
        &bytes,
        &ConversionContext::new().with_cancellation(flag.handle()),
    );
    assert_eq!(cancelled, ConversionOutcome::Cancelled);

    for (budgets, expected) in [
        (
            ConversionBudgets {
                max_source_bytes: bytes.len() as u64 - 1,
                ..ConversionBudgets::default()
            },
            BudgetKind::SourceBytes,
        ),
        (
            ConversionBudgets {
                max_expanded_bytes: 64,
                ..ConversionBudgets::default()
            },
            BudgetKind::ExpandedBytes,
        ),
        (
            ConversionBudgets {
                max_archive_entries: 2,
                ..ConversionBudgets::default()
            },
            BudgetKind::ArchiveEntries,
        ),
        (
            ConversionBudgets {
                max_items: 1,
                ..ConversionBudgets::default()
            },
            BudgetKind::Items,
        ),
    ] {
        let outcome = convert_with_context(&bytes, &ConversionContext::new().with_budgets(budgets));
        assert!(
            matches!(
                outcome,
                ConversionOutcome::OverBudget { budget, .. } if budget == expected
            ),
            "expected {expected:?}, got {outcome:?}"
        );
    }

    let output_limited = convert_with_context(
        &bytes,
        &ConversionContext::new().with_budgets(ConversionBudgets {
            max_output_chars: 12,
            ..ConversionBudgets::default()
        }),
    );
    let document = output_limited
        .document()
        .expect("bounded output remains explicit partial success");
    assert_eq!(document.completeness(), Completeness::Partial);
    assert!(!document.omissions().is_empty());
}

#[test]
fn spine_boundaries_keep_headingless_chapters_in_separate_deterministic_chunks() {
    let bytes = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"one\" href=\"one.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"two\" href=\"two.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"one\"/><itemref idref=\"two\"/></spine></package>",
        ),
        ("OPS/one.xhtml", b"<html><body><p>one</p></body></html>"),
        ("OPS/two.xhtml", b"<html><body><p>two</p></body></html>"),
    ]);
    let outcome = convert(&bytes);
    let document = outcome.document().expect("converted EPUB");

    let first = Chunker::default().chunk(document);
    let second = Chunker::default().chunk(document);
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
    assert_eq!(first[0].embedding_input, "one");
    assert_eq!(first[1].embedding_input, "two");

    let json = serde_json::to_string(&first[1].provenance).expect("serialize provenance");
    assert_eq!(
        json,
        r#"{"kind":"exact","value":{"kind":"epubText","spine_index":1,"start_line":1,"end_line":1}}"#
    );
}

#[test]
fn percent_encoded_rootfile_and_spine_paths_resolve_utf8_zip_entries() {
    let bytes = epub(&[
        (
            "META-INF/container.xml",
            b"<container><rootfiles><rootfile full-path=\"OPS/package%20document.opf\"/></rootfiles></container>",
        ),
        (
            "OPS/package document.opf",
            b"<package><manifest><item id=\"one\" href=\"text/chapter%201.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"two\" href=\"text/caf%C3%A9.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"one\"/><itemref idref=\"two\"/></spine></package>",
        ),
        (
            "OPS/text/chapter 1.xhtml",
            b"<html><body><p>Encoded space.</p></body></html>",
        ),
        (
            "OPS/text/café.xhtml",
            b"<html><body><p>Encoded UTF-8.</p></body></html>",
        ),
    ]);

    let outcome = convert(&bytes);
    let document = outcome.document().expect("percent-encoded EPUB");
    let text = document
        .units()
        .iter()
        .map(|unit| unit.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(text, ["Encoded space.", "Encoded UTF-8."]);
}

#[test]
fn percent_decoding_cannot_hide_unsafe_or_malformed_spine_paths() {
    for href in [
        "%2e%2e/escape.xhtml",
        "%2Fabsolute.xhtml",
        "text%5Cescape.xhtml",
        "https%3A//example.test/chapter.xhtml",
        "chapter%2.xhtml",
        "chapter%FF.xhtml",
    ] {
        let opf = format!(
            "<package><manifest><item id=\"chapter\" href=\"{href}\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>"
        );
        let bytes = epub(&[
            ("META-INF/container.xml", CONTAINER),
            ("OPS/package.opf", opf.as_bytes()),
        ]);
        assert!(
            matches!(convert(&bytes), ConversionOutcome::Malformed { .. }),
            "unsafe path was accepted: {href}"
        );
    }
}

#[test]
fn font_only_encryption_does_not_block_readable_spine_text() {
    let bytes = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "META-INF/encryption.xml",
            br#"<enc:encryption xmlns:enc="http://www.w3.org/2001/04/xmlenc#">
  <enc:EncryptedData>
    <enc:CipherData><enc:CipherReference URI="OPS/fonts/book%20font.woff2"/></enc:CipherData>
  </enc:EncryptedData>
</enc:encryption>"#,
        ),
        (
            "OPS/package.opf",
            b"<package><manifest><item id=\"chapter\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/><item id=\"font\" href=\"fonts/book%20font.woff2\" media-type=\"font/woff2\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>",
        ),
        (
            "OPS/chapter.xhtml",
            b"<html><body><p>Readable chapter.</p></body></html>",
        ),
        ("OPS/fonts/book font.woff2", b"obfuscated font bytes"),
    ]);

    let outcome = convert(&bytes);
    assert_eq!(
        outcome
            .document()
            .expect("font encryption is ignored")
            .units()[0]
            .text,
        "Readable chapter."
    );
}

#[test]
fn encrypted_spine_xhtml_and_malformed_encryption_metadata_are_typed() {
    let opf = b"<package><manifest><item id=\"chapter\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest><spine><itemref idref=\"chapter\"/></spine></package>";
    let chapter = b"<html><body><p>Encrypted chapter.</p></body></html>";
    let encrypted = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "META-INF/encryption.xml",
            b"<encryption><EncryptedData><CipherData><CipherReference URI=\"OPS/chapter.xhtml\"/></CipherData></EncryptedData></encryption>",
        ),
        ("OPS/package.opf", opf),
        ("OPS/chapter.xhtml", chapter),
    ]);
    assert!(matches!(
        convert(&encrypted),
        ConversionOutcome::Encrypted { .. }
    ));

    let malformed = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "META-INF/encryption.xml",
            b"<encryption><EncryptedData><CipherData></EncryptedData></encryption>",
        ),
        ("OPS/package.opf", opf),
        ("OPS/chapter.xhtml", chapter),
    ]);
    assert!(matches!(
        convert(&malformed),
        ConversionOutcome::Malformed { .. }
    ));

    let unsafe_target = epub(&[
        ("META-INF/container.xml", CONTAINER),
        (
            "META-INF/encryption.xml",
            b"<encryption><EncryptedData><CipherData><CipherReference URI=\"%2e%2e/escape.xhtml\"/></CipherData></EncryptedData></encryption>",
        ),
        ("OPS/package.opf", opf),
        ("OPS/chapter.xhtml", chapter),
        ("escape.xhtml", b"not a valid encryption target"),
    ]);
    assert!(matches!(
        convert(&unsafe_target),
        ConversionOutcome::Malformed { .. }
    ));
}
