use std::fs;
use std::sync::Arc;

use fm_vfs::ProviderRegistry;
use fm_vfs_local::LocalFileSystemProvider;

use super::RemoteSandbox;
use crate::agent_host::sandbox::FileCall;
use crate::agent_host::sandbox::ToolOutput;

/// The local provider stands in for any provider: the tools only use the provider interface.
fn remote() -> (tempfile::TempDir, RemoteSandbox) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("b.txt"), "beta\nneedle\n").unwrap();
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "fn main() {}").unwrap();
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(LocalFileSystemProvider::new()));
    let uri = url::Url::from_directory_path(dir.path().canonicalize().unwrap())
        .unwrap()
        .to_string();
    let sandbox = RemoteSandbox::new(&registry, &uri).unwrap();
    (dir, sandbox)
}

async fn run(sandbox: &RemoteSandbox, call: FileCall) -> ToolOutput {
    sandbox.run(&call).await
}

#[tokio::test]
async fn list_dir_names_entries_sorted_with_folders_marked() {
    let (_dir, sandbox) = remote();
    let output = run(&sandbox, FileCall::ListDir { path: ".".into() }).await;
    assert_eq!(output.step, "Listed .");
    assert_eq!(output.text, "a.txt\nb.txt\nsrc/");
    let nested = run(&sandbox, FileCall::ListDir { path: "src".into() }).await;
    assert_eq!(nested.text, "lib.rs");
}

#[tokio::test]
async fn read_file_returns_redacted_bounded_text() {
    let (dir, sandbox) = remote();
    fs::write(dir.path().join("conf.txt"), "TOKEN=abc\nname=x\n").unwrap();
    let output = run(
        &sandbox,
        FileCall::ReadFile {
            path: "conf.txt".into(),
            offset: None,
        },
    )
    .await;
    assert_eq!(output.step, "Read conf.txt");
    assert_eq!(output.text, "TOKEN=[redacted]\nname=x\n");

    fs::write(dir.path().join("big.txt"), "z".repeat(40_000)).unwrap();
    let big = run(
        &sandbox,
        FileCall::ReadFile {
            path: "big.txt".into(),
            offset: None,
        },
    )
    .await;
    assert!(big.text.len() < 33_000);
    assert!(big.text.contains("[truncated at byte"));
}

#[tokio::test]
async fn paths_outside_credential_folders_and_secret_files_are_refused() {
    let (dir, sandbox) = remote();
    fs::write(dir.path().join(".env"), "A=1").unwrap();
    fs::create_dir(dir.path().join(".ssh")).unwrap();
    fs::write(dir.path().join(".ssh/config"), "Host x").unwrap();
    for path in [
        "../x",
        "/etc/hosts",
        "missing.txt",
        ".env",
        ".ssh/config",
        "src/../a.txt",
    ] {
        let output = run(
            &sandbox,
            FileCall::ReadFile {
                path: path.into(),
                offset: None,
            },
        )
        .await;
        assert!(output.text.starts_with("Error:"), "{path}: {}", output.text);
    }
    let missing = run(
        &sandbox,
        FileCall::ReadFile {
            path: "missing.txt".into(),
            offset: None,
        },
    )
    .await;
    let outside = run(
        &sandbox,
        FileCall::ReadFile {
            path: "../x".into(),
            offset: None,
        },
    )
    .await;
    assert_eq!(missing.text, "Error: missing.txt is not available.");
    assert_eq!(outside.text, "Error: ../x is not available.");
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_is_not_followed() {
    let (dir, sandbox) = remote();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("o.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("a.txt"), dir.path().join("a-link")).unwrap();
    for path in ["link/o.txt", "a-link"] {
        let output = run(
            &sandbox,
            FileCall::ReadFile {
                path: path.into(),
                offset: None,
            },
        )
        .await;
        assert!(output.text.starts_with("Error:"), "{path}: {}", output.text);
    }
}

#[tokio::test]
async fn a_folder_is_not_a_file_and_a_file_is_not_a_folder() {
    let (_dir, sandbox) = remote();
    assert!(
        run(
            &sandbox,
            FileCall::ReadFile {
                path: "src".into(),
                offset: None
            }
        )
        .await
        .text
        .starts_with("Error:")
    );
    assert!(
        run(
            &sandbox,
            FileCall::ListDir {
                path: "a.txt".into()
            }
        )
        .await
        .text
        .starts_with("Error:")
    );
}

#[tokio::test]
async fn grep_is_not_available_for_remote_folders() {
    let (_dir, sandbox) = remote();
    let output = run(
        &sandbox,
        FileCall::Grep {
            pattern: "x".into(),
            path: None,
        },
    )
    .await;
    assert!(
        output.text.starts_with("Error: grep is not available"),
        "{}",
        output.text
    );
}

#[tokio::test]
async fn named_files_can_be_read_for_attaching() {
    let (_dir, sandbox) = remote();
    assert_eq!(sandbox.read_text("a.txt").await, Some("alpha".into()));
    assert_eq!(sandbox.read_text("../x").await, None);
}
