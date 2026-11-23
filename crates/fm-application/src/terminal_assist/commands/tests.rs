use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use super::*;

fn registry() -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers.register(Arc::new(LocalFileSystemProvider));
    providers
}

fn url(path: &std::path::Path) -> String {
    url::Url::from_directory_path(path.canonicalize().unwrap())
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn files_under_the_root_are_listed_with_subfolders_and_without_build_output() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src/deep")).unwrap();
    std::fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    std::fs::create_dir_all(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join("README.md"), "").unwrap();
    std::fs::write(root.path().join("src/deep/main.rs"), "").unwrap();
    std::fs::write(root.path().join("node_modules/pkg/index.js"), "").unwrap();

    let listed = list_files(&registry(), &url(root.path())).await;
    assert_eq!(
        listed.files,
        ["README.md", "src/", "src/deep/", "src/deep/main.rs"]
    );
    assert!(!listed.truncated);

    // A file created later is listed on the next call.
    std::fs::write(root.path().join("src/new.rs"), "").unwrap();
    let listed = list_files(&registry(), &url(root.path())).await;
    assert!(listed.files.contains(&"src/new.rs".to_owned()));
}
