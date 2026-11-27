use std::fs;
use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use super::{FileWriter, WriteError, explicit_destination, valid_name};

fn writer() -> (tempfile::TempDir, FileWriter) {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(LocalFileSystemProvider::new()));
    let uri = url::Url::from_directory_path(dir.path().canonicalize().unwrap())
        .unwrap()
        .to_string();
    let writer = FileWriter::open(&registry, &uri, &uri).unwrap();
    (dir, writer)
}

#[tokio::test]
async fn creates_the_file_in_the_working_folder() {
    let (dir, writer) = writer();
    let created = writer.create("a.md", "# A\n", true).await.unwrap();
    assert_eq!(created.name, "a.md");
    assert_eq!(created.bytes, 4);
    assert_eq!(
        fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "# A\n"
    );
    assert_eq!(created.step(), "Created ./a.md (4 B)");
}

#[tokio::test]
async fn a_taken_name_gets_a_number_and_the_original_is_untouched() {
    let (dir, writer) = writer();
    writer.create("a.md", "first", true).await.unwrap();
    let second = writer.create("a.md", "second", true).await.unwrap();
    let third = writer.create("a.md", "third", true).await.unwrap();
    assert_eq!(second.name, "a-2.md");
    assert_eq!(third.name, "a-3.md");
    assert!(second.result().contains("a-2.md"));
    assert_eq!(
        fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "first"
    );
}

#[tokio::test]
async fn an_exact_destination_never_creates_a_suffixed_copy() {
    let (dir, writer) = writer();
    fs::write(dir.path().join("a.md"), "existing").unwrap();
    assert_eq!(
        writer.create("a.md", "replacement", false).await,
        Err(WriteError::Exists("a.md".to_owned()))
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("a.md")).unwrap(),
        "existing"
    );
    assert!(!dir.path().join("a-2.md").exists());
}

#[test]
fn implicit_approval_requires_a_direct_request_for_this_filename() {
    assert!(explicit_destination(
        "Write the report to `./a.md`.",
        "a.md"
    ));
    for question in [
        "Read a.md and summarize it.",
        "Do not write to a.md.",
        "Write to sub/a.md.",
        "Write to /a.md.",
        "Write the report to ab.md.",
    ] {
        assert!(!explicit_destination(question, "a.md"), "{question}");
    }
}

#[tokio::test]
async fn refuses_names_that_leave_the_folder_or_start_programs() {
    let (dir, writer) = writer();
    for name in [
        "../a.md",
        "sub/a.md",
        "/etc/a.md",
        "..\\a.md",
        ".zshrc",
        ".hidden.md",
        "script.sh",
        "run.py",
        "noextension",
        "a.md.exe",
        ".md",
        "id_rsa.txt",
        "my-secret.md",
        "a\nb.md",
        " a.md",
    ] {
        assert_eq!(
            writer.create(name, "x", true).await,
            Err(WriteError::Name(name.to_owned())),
            "{name}"
        );
    }
    assert!(!dir.path().join("summaries").exists());
}

#[test]
fn accepts_plain_markdown_and_text_names() {
    for name in ["a.md", "Q3 report.md", "notes.TXT", "x-2.md", "über.md"] {
        assert!(valid_name(name), "{name}");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_dangling_symlink_at_the_target_is_not_followed() {
    let (dir, writer) = writer();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path().join("planted.md"), dir.path().join("a.md")).unwrap();
    let created = writer.create("a.md", "x", true).await.unwrap();
    assert_eq!(created.name, "a-2.md");
    assert!(!outside.path().join("planted.md").exists());
}

#[tokio::test]
async fn enforces_the_file_size_limit() {
    let (_dir, writer) = writer();
    let big = "x".repeat(super::MAX_FILE_BYTES + 1);
    assert_eq!(
        writer.create("a.md", &big, true).await,
        Err(WriteError::TooLarge)
    );
    assert!(
        writer
            .create("b.md", &"x".repeat(super::MAX_FILE_BYTES), true)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn secrets_in_the_content_are_replaced_and_counted() {
    let (dir, writer) = writer();
    let created = writer
        .create("a.md", "host: x\npassword=hunter2\n", true)
        .await
        .unwrap();
    assert_eq!(created.redacted, 1);
    assert!(created.result().contains("1 value(s)"));
    let text = fs::read_to_string(dir.path().join("a.md")).unwrap();
    assert!(!text.contains("hunter2"));
}
