use std::io::{Cursor, Write as _};
use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use fm_semantic_conversion::{BudgetKind, ConversionOutcome};

use super::{Documents, converted_text, is_document, window};
use crate::agent_host::sandbox::FileCall;
use crate::agent_host::sandbox::{MAX_READ_BYTES, ToolHost};

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
    assert!(is_document("page.html") && is_document("old.HTM"));
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

    let call = FileCall::ReadFile {
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
            &FileCall::ReadFile {
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
async fn a_saved_web_page_is_read_as_its_words() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<html><head><style>p{color:red}</style><script>track()</script></head><body>\
         <nav><a href=\"/\">Home</a></nav><article><h1>Decoy warfare</h1>\
         <p>Decoys &amp; deception win time.</p></article></body></html>",
    )
    .unwrap();
    let providers = registry();
    let working = url::Url::from_directory_path(dir.path())
        .unwrap()
        .to_string();
    let host = ToolHost::open(&providers, &working).unwrap();

    let output = Documents::new(&providers)
        .run(
            &host,
            &FileCall::ReadFile {
                path: "page.html".into(),
                offset: None,
            },
        )
        .await
        .expect("a document read");
    assert!(output.text.contains("Decoy warfare"), "{}", output.text);
    assert!(output.text.contains("Decoys & deception win time."));
    for markup in ["<p>", "track()", "color:red"] {
        assert!(!output.text.contains(markup), "{markup}: {}", output.text);
    }
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

    let plain = FileCall::ReadFile {
        path: "notes.txt".into(),
        offset: None,
    };
    assert!(documents.run(&host, &plain).await.is_none());

    let missing = FileCall::ReadFile {
        path: "missing.epub".into(),
        offset: None,
    };
    let output = documents.run(&host, &missing).await.unwrap();
    assert!(output.text.starts_with("Error:"), "{}", output.text);

    let outside = FileCall::ReadFile {
        path: "../secret.epub".into(),
        offset: None,
    };
    let output = documents.run(&host, &outside).await.unwrap();
    assert!(output.text.starts_with("Error:"));
}

#[tokio::test]
async fn a_document_inside_a_skill_folder_is_read_as_text() {
    use fm_agent::SkillDocuments as _;

    let outer = tempfile::tempdir().unwrap();
    let skill = outer.path().join("skill");
    std::fs::create_dir(&skill).unwrap();
    std::fs::write(skill.join("guide.epub"), epub(3)).unwrap();
    std::fs::write(skill.join("notes.txt"), "plain").unwrap();
    let documents = super::SkillFolderDocuments::new(&registry());

    let local = fm_agent::StoreHandle::local();
    let path = |name: &str| skill.join(name).to_string_lossy().into_owned();
    let text = documents
        .read(&local, &path("guide.epub"), 0)
        .await
        .expect("a document")
        .unwrap();
    assert!(text.contains("Paragraph number 0000"), "{text}");
    // Plain files are the skill reader's business. (The path itself was checked by the harness;
    // fm-agent's tests cover that a path outside the skill never reaches the converter.)
    assert!(
        documents
            .read(&local, &path("notes.txt"), 0)
            .await
            .is_none()
    );
}

#[test]
fn a_conversion_over_a_budget_names_the_budget() {
    let reason = |budget, limit| {
        converted_text(ConversionOutcome::OverBudget { budget, limit }, "book.pdf")
            .expect_err("over budget")
            .to_string()
    };
    let slow = reason(BudgetKind::Time, 95_000);
    assert!(
        slow.contains("book.pdf: converting it took longer than 95 s"),
        "{slow}"
    );
    assert!(!slow.contains("too large"), "{slow}");
    let big = reason(BudgetKind::SourceBytes, 64 * 1024 * 1024);
    assert!(
        big.contains("larger than the 64 MB the converter reads"),
        "{big}"
    );
    let pages = reason(BudgetKind::Items, 4096);
    assert!(pages.contains("more than the 4096 pages"), "{pages}");
    let deep = reason(BudgetKind::NestingDepth, 128);
    assert!(deep.contains("nesting depth limit (128)"), "{deep}");
}
