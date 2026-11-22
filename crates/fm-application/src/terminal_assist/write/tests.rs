use std::fs;
use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use super::{FileWriter, WriteError, valid_name};

fn writer() -> (tempfile::TempDir, FileWriter) {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(LocalFileSystemProvider::new()));
    let uri = url::Url::from_directory_path(dir.path().canonicalize().unwrap())
        .unwrap()
        .to_string();
    let writer = FileWriter::open(&registry, &uri).unwrap();
    (dir, writer)
}

#[tokio::test]
async fn creates_the_folder_and_the_file() {
    let (dir, writer) = writer();
    let created = writer.create("a.md", "# A\n").await.unwrap();
    assert_eq!(created.name, "a.md");
    assert_eq!(created.bytes, 4);
    assert_eq!(
        fs::read_to_string(dir.path().join("summaries/a.md")).unwrap(),
        "# A\n"
    );
    assert_eq!(created.step(), "Created summaries/a.md (4 B)");
}

#[tokio::test]
async fn a_taken_name_gets_a_number_and_the_original_is_untouched() {
    let (dir, writer) = writer();
    writer.create("a.md", "first").await.unwrap();
    let second = writer.create("a.md", "second").await.unwrap();
    let third = writer.create("a.md", "third").await.unwrap();
    assert_eq!(second.name, "a-2.md");
    assert_eq!(third.name, "a-3.md");
    assert!(second.result().contains("summaries/a-2.md"));
    assert_eq!(
        fs::read_to_string(dir.path().join("summaries/a.md")).unwrap(),
        "first"
    );
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
            writer.create(name, "x").await,
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
async fn a_symlink_output_folder_is_refused() {
    let (dir, writer) = writer();
    let target = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(target.path(), dir.path().join("summaries")).unwrap();
    assert_eq!(writer.create("a.md", "x").await, Err(WriteError::Folder));
    assert!(fs::read_dir(target.path()).unwrap().next().is_none());
}

#[tokio::test]
async fn a_file_named_like_the_folder_is_refused() {
    let (dir, writer) = writer();
    fs::write(dir.path().join("summaries"), "x").unwrap();
    assert_eq!(writer.create("a.md", "x").await, Err(WriteError::Folder));
}

#[cfg(unix)]
#[tokio::test]
async fn a_dangling_symlink_at_the_target_is_not_followed() {
    let (dir, writer) = writer();
    fs::create_dir(dir.path().join("summaries")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("planted.md"),
        dir.path().join("summaries/a.md"),
    )
    .unwrap();
    let created = writer.create("a.md", "x").await.unwrap();
    assert_eq!(created.name, "a-2.md");
    assert!(!outside.path().join("planted.md").exists());
}

#[tokio::test]
async fn enforces_the_file_size_limit() {
    let (_dir, writer) = writer();
    let big = "x".repeat(super::MAX_FILE_BYTES + 1);
    assert_eq!(writer.create("a.md", &big).await, Err(WriteError::TooLarge));
    assert!(
        writer
            .create("b.md", &"x".repeat(super::MAX_FILE_BYTES))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn secrets_in_the_content_are_replaced_and_counted() {
    let (dir, writer) = writer();
    let created = writer
        .create("a.md", "host: x\npassword=hunter2\n")
        .await
        .unwrap();
    assert_eq!(created.redacted, 1);
    assert!(created.result().contains("1 value(s)"));
    let text = fs::read_to_string(dir.path().join("summaries/a.md")).unwrap();
    assert!(!text.contains("hunter2"));
}
