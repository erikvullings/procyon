use std::fs;
use std::sync::Arc;

use fm_domain::{Location, ProviderId};
use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;
use tokio_util::sync::CancellationToken;

use super::{NotesError, NotesStore, is_note_name};

pub(crate) fn registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(LocalFileSystemProvider::new()));
    registry
}

pub(crate) fn location_of(path: &std::path::Path) -> Location {
    Location::new(
        ProviderId::new("local"),
        url::Url::from_directory_path(path.canonicalize().unwrap())
            .unwrap()
            .to_string(),
    )
}

async fn store_with_notes() -> (tempfile::TempDir, NotesStore) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("notes");
    fs::create_dir_all(root.join("ops/tls")).unwrap();
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::create_dir_all(root.join(".ssh")).unwrap();
    fs::write(root.join("b note.md"), "# B\n").unwrap();
    fs::write(root.join("a.md"), "# A\n").unwrap();
    fs::write(root.join("ops/tls/nginx.markdown"), "# nginx\n").unwrap();
    fs::write(root.join("ops/readme.txt"), "plain").unwrap();
    fs::write(root.join("image.png"), [0u8, 1, 2]).unwrap();
    fs::write(root.join(".obsidian/workspace.md"), "hidden").unwrap();
    fs::write(root.join(".ssh/config.md"), "secret").unwrap();
    fs::write(root.join("server.key"), "-----BEGIN PRIVATE KEY-----").unwrap();
    fs::write(dir.path().join("outside.md"), "outside").unwrap();
    let store = NotesStore::open(&registry(), &location_of(&root))
        .await
        .unwrap();
    (dir, store)
}

#[tokio::test]
async fn a_walk_lists_note_files_sorted_and_skips_hidden_credential_and_other_files() {
    let (_dir, store) = store_with_notes().await;
    let paths: Vec<String> = store
        .walk(&CancellationToken::new())
        .await
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert_eq!(
        paths,
        [
            "a.md",
            "b note.md",
            "ops/readme.txt",
            "ops/tls/nginx.markdown"
        ]
    );
}

#[tokio::test]
async fn reading_a_note_inside_the_folder_works_with_any_separator() {
    let (_dir, store) = store_with_notes().await;
    let note = store.read("ops\\tls/./nginx.markdown").await.unwrap();
    assert_eq!(note.path, "ops/tls/nginx.markdown");
    assert_eq!(note.text, "# nginx\n");
    assert!(note.location.uri.ends_with("/ops/tls/nginx.markdown"));
    assert_eq!(store.read("b note.md").await.unwrap().text, "# B\n");
}

#[tokio::test]
async fn paths_that_escape_or_hide_are_not_available() {
    let (dir, store) = store_with_notes().await;
    let outside = dir.path().join("outside.md").display().to_string();
    for path in [
        "../outside.md",
        "ops/../../outside.md",
        outside.as_str(),
        "",
        "  ",
        ".obsidian/workspace.md",
        ".ssh/config.md",
        "server.key",
        "image.png",
        "missing.md",
        "ops",
        "C:/x.md",
    ] {
        assert!(
            matches!(store.read(path).await, Err(NotesError::NotAvailable(_))),
            "{path} should not be available"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_never_followed() {
    let (dir, store) = store_with_notes().await;
    let root = dir.path().join("notes");
    std::os::unix::fs::symlink(dir.path().join("outside.md"), root.join("link.md")).unwrap();
    std::os::unix::fs::symlink(dir.path(), root.join("up")).unwrap();
    assert!(store.read("link.md").await.is_err());
    assert!(store.read("up/outside.md").await.is_err());
    let paths: Vec<String> = store
        .walk(&CancellationToken::new())
        .await
        .into_iter()
        .map(|file| file.path)
        .collect();
    assert!(
        !paths
            .iter()
            .any(|path| path.contains("link") || path.starts_with("up"))
    );
}

#[tokio::test]
async fn a_location_maps_back_to_its_path_only_inside_the_folder() {
    let (dir, store) = store_with_notes().await;
    let note = store.read("b note.md").await.unwrap();
    assert_eq!(
        store.path_of(&note.location).await.as_deref(),
        Some("b note.md")
    );
    let nested = store.read("ops/tls/nginx.markdown").await.unwrap();
    assert_eq!(
        store.path_of(&nested.location).await.as_deref(),
        Some("ops/tls/nginx.markdown")
    );
    let outside = Location::new(
        ProviderId::new("local"),
        url::Url::from_file_path(dir.path().canonicalize().unwrap().join("outside.md"))
            .unwrap()
            .to_string(),
    );
    assert_eq!(store.path_of(&outside).await, None);
    let mut hidden = store.root().clone();
    hidden = hidden
        .join(".obsidian")
        .unwrap()
        .join("workspace.md")
        .unwrap();
    assert_eq!(store.path_of(&hidden).await, None);
}

#[tokio::test]
async fn opening_needs_an_existing_folder() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("file.md"), "x").unwrap();
    let missing = location_of(dir.path()).join("missing").unwrap();
    assert!(matches!(
        NotesStore::open(&registry(), &missing).await,
        Err(NotesError::RootUnavailable)
    ));
    let file = location_of(dir.path()).join("file.md").unwrap();
    assert!(NotesStore::open(&registry(), &file).await.is_err());
}

#[tokio::test]
async fn oversized_and_binary_notes_are_refused() {
    let (dir, store) = store_with_notes().await;
    let root = dir.path().join("notes");
    fs::write(root.join("binary.md"), [b'a', 0, b'b']).unwrap();
    assert!(matches!(
        store.read("binary.md").await,
        Err(NotesError::NotText(_))
    ));
    let location = store.locate("a.md").await.unwrap();
    assert!(matches!(
        store.read_text(&location, 2, "a.md").await,
        Err(NotesError::NotText(_))
    ));
}

#[test]
fn note_names_need_a_note_extension_and_a_stem() {
    assert!(is_note_name("a.md"));
    assert!(is_note_name("A.MARKDOWN"));
    assert!(is_note_name("x.txt"));
    assert!(!is_note_name(".md"));
    assert!(!is_note_name("md"));
    assert!(!is_note_name("x.pdf"));
}
