use std::io::{Cursor, Write as _};
use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use super::{Documents, is_document, window};
use crate::terminal_assist::protocol::ToolCall;
use crate::terminal_assist::tools::{MAX_READ_BYTES, ToolHost};

fn registry() -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    providers
}

fn epub(paragraphs: usize) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file(
            "mimetype",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    archive.write_all(b"application/epub+zip").unwrap();
    let body: String = (0..paragraphs)
        .map(|n| format!("<p>Paragraph number {n:04} of the deception report.</p>"))
        .collect();
    let chapter = format!("<html><body><h1>Chapter</h1>{body}</body></html>");
    for (name, content) in [
        (
            "META-INF/container.xml",
            r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_owned(),
        ),
        (
            "EPUB/content.opf",
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_owned(),
        ),
        ("EPUB/c.xhtml", chapter),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }
    archive.finish().unwrap().into_inner()
}

#[test]
fn documents_are_recognised_by_extension() {
    assert!(is_document("book.epub"));
    assert!(is_document("Report.PDF"));
    assert!(is_document("dir/a.docx"));
    assert!(!is_document("notes.md"));
    assert!(!is_document("epub"));
}

#[test]
fn a_long_text_is_read_in_windows_that_continue() {
    let text = "é".repeat(MAX_READ_BYTES);
    let first = window(&text, 0);
    assert!(first.contains("\"offset\":"));
    let marker = first.rfind("\n[truncated").unwrap();
    let end: usize = first[marker..]
        .split("at byte ")
        .nth(1)
        .and_then(|rest| rest.split(':').next())
        .and_then(|number| number.parse().ok())
        .unwrap();
    let rest = window(&text, end);
    assert!(first[..marker].len() + rest.len() >= text.len());
    assert!(window("short", 0) == "short");
    assert_eq!(window("short", 99), "");
}

#[tokio::test]
async fn an_epub_is_read_as_text_and_continues_with_an_offset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("book.epub"), epub(700)).unwrap();
    let providers = registry();
    let working = url::Url::from_directory_path(dir.path())
        .unwrap()
        .to_string();
    let host = ToolHost::open(&providers, &working).unwrap();
    let documents = Documents::new(&providers);

    let call = ToolCall::ReadFile {
        path: "book.epub".into(),
        offset: None,
    };
    let output = documents.run(&host, &call).await.expect("a document read");
    assert_eq!(output.step, "Read book.epub");
    assert!(
        output.text.contains("Paragraph number 0000"),
        "{}",
        output.text
    );
    assert!(output.text.contains("[truncated at byte"));

    let end: usize = output
        .text
        .split("\"offset\":")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|number| number.parse().ok())
        .unwrap();
    let later = documents
        .run(
            &host,
            &ToolCall::ReadFile {
                path: "book.epub".into(),
                offset: Some(end),
            },
        )
        .await
        .unwrap();
    assert!(!later.text.contains("Paragraph number 0000"));
    assert!(
        later.text.contains("Paragraph number 0699"),
        "{}",
        later.text
    );
    assert_eq!(
        documents.conversions(),
        1,
        "the second window must come from the cache"
    );
}

#[tokio::test]
async fn plain_files_and_missing_documents_are_handled_apart() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "hello").unwrap();
    let providers = registry();
    let working = url::Url::from_directory_path(dir.path())
        .unwrap()
        .to_string();
    let host = ToolHost::open(&providers, &working).unwrap();
    let documents = Documents::new(&providers);

    let plain = ToolCall::ReadFile {
        path: "notes.txt".into(),
        offset: None,
    };
    assert!(documents.run(&host, &plain).await.is_none());

    let missing = ToolCall::ReadFile {
        path: "missing.epub".into(),
        offset: None,
    };
    let output = documents.run(&host, &missing).await.unwrap();
    assert!(output.text.starts_with("Error:"), "{}", output.text);

    let outside = ToolCall::ReadFile {
        path: "../secret.epub".into(),
        offset: None,
    };
    let output = documents.run(&host, &outside).await.unwrap();
    assert!(output.text.starts_with("Error:"));
}
