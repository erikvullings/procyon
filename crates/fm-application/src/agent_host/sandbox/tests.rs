use std::fs;

use super::{FileCall, Sandbox, ToolOutput};

fn sandbox() -> (tempfile::TempDir, Sandbox) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("b.txt"), "beta\nneedle here\n").unwrap();
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "fn needle() {}\n").unwrap();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::write(dir.path().join("node_modules/x.js"), "needle").unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    (dir, sandbox)
}

#[test]
fn list_dir_names_entries_sorted_with_folders_marked() {
    let (_dir, sandbox) = sandbox();
    let ToolOutput { step, text } = sandbox.run(&FileCall::ListDir { path: ".".into() });
    assert_eq!(step, "Listed .");
    assert_eq!(text, "a.txt\nb.txt\nnode_modules/\nsrc/");
}

#[test]
fn paths_outside_the_folder_are_refused() {
    let (_dir, sandbox) = sandbox();
    let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
        path: "../secret".into(),
        offset: None,
    });
    assert!(text.starts_with("Error:"), "{text}");
    let ToolOutput { text, .. } = sandbox.run(&FileCall::ListDir {
        path: "/etc".into(),
    });
    assert!(text.starts_with("Error:"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_symlink_leading_out_is_refused() {
    let (dir, sandbox) = sandbox();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("o.txt"), "outside").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
        path: "link/o.txt".into(),
        offset: None,
    });
    assert!(text.starts_with("Error:"), "{text}");
}

#[test]
fn read_file_returns_redacted_bounded_text() {
    let (dir, sandbox) = sandbox();
    fs::write(dir.path().join("conf.txt"), "TOKEN=abc\nname=x\n").unwrap();
    let ToolOutput { step, text } = sandbox.run(&FileCall::ReadFile {
        path: "conf.txt".into(),
        offset: None,
    });
    assert_eq!(step, "Read conf.txt");
    assert_eq!(text, "TOKEN=[redacted]\nname=x\n");

    fs::write(dir.path().join("big.txt"), "z".repeat(40_000)).unwrap();
    let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
        path: "big.txt".into(),
        offset: None,
    });
    assert!(text.len() < 33_000);
    assert!(text.contains("[truncated at byte"));

    // The rest of a cut-off file is reachable with `offset`.
    let mut numbered = String::new();
    for n in 0..8_000 {
        numbered.push_str(&format!("{n:05}\n"));
    }
    fs::write(dir.path().join("long.txt"), &numbered).unwrap();
    let ToolOutput { text: first, .. } = sandbox.run(&FileCall::ReadFile {
        path: "long.txt".into(),
        offset: None,
    });
    assert!(first.contains("\"offset\":32768"));
    let ToolOutput { text: rest, .. } = sandbox.run(&FileCall::ReadFile {
        path: "long.txt".into(),
        offset: Some(32_768),
    });
    assert!(rest.ends_with("07999\n"));
    assert!(!rest.contains("[truncated"));
}

#[test]
fn secret_looking_and_binary_files_are_not_read() {
    let (dir, sandbox) = sandbox();
    fs::write(dir.path().join(".env"), "A=1").unwrap();
    fs::write(dir.path().join("id_rsa"), "key").unwrap();
    fs::write(dir.path().join("bin.dat"), [0u8, 1, 2, 0]).unwrap();
    for name in [".env", "id_rsa", "bin.dat"] {
        let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
            path: name.into(),
            offset: None,
        });
        assert!(text.starts_with("Error:"), "{name}: {text}");
    }
    assert_eq!(sandbox.read_text(".env"), None);
    assert_eq!(sandbox.read_text("a.txt"), Some("alpha".into()));
}

#[test]
fn grep_finds_lines_and_skips_build_folders() {
    let (_dir, sandbox) = sandbox();
    let ToolOutput { step, text } = sandbox.run(&FileCall::Grep {
        pattern: "NEEDLE".into(),
        path: None,
    });
    assert_eq!(step, "Searched for NEEDLE");
    assert_eq!(text, "b.txt:2: needle here\nsrc/lib.rs:1: fn needle() {}");
    let ToolOutput { text: none, .. } = sandbox.run(&FileCall::Grep {
        pattern: "absent".into(),
        path: Some("src".into()),
    });
    assert_eq!(none, "No matches.");
}

#[test]
fn a_missing_path_and_an_outside_path_read_the_same() {
    let (_dir, sandbox) = sandbox();
    let ToolOutput { text: missing, .. } = sandbox.run(&FileCall::ReadFile {
        path: "nope.txt".into(),
        offset: None,
    });
    let ToolOutput { text: outside, .. } = sandbox.run(&FileCall::ReadFile {
        path: "/etc/hosts".into(),
        offset: None,
    });
    assert_eq!(missing, "Error: nope.txt is not available.");
    assert_eq!(outside, "Error: /etc/hosts is not available.");
}

#[test]
fn more_credential_files_and_folders_are_refused() {
    let (dir, sandbox) = sandbox();
    fs::write(dir.path().join(".pgpass"), "h:5432:d:u:p").unwrap();
    fs::write(dir.path().join("state.tfstate"), "{}").unwrap();
    fs::create_dir(dir.path().join(".aws")).unwrap();
    fs::write(dir.path().join(".aws/credentials"), "key").unwrap();
    for name in [".pgpass", "state.tfstate", ".aws/credentials"] {
        let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
            path: name.into(),
            offset: None,
        });
        assert!(text.starts_with("Error:"), "{name}: {text}");
    }
    let ToolOutput { text, .. } = sandbox.run(&FileCall::Grep {
        pattern: "key".into(),
        path: None,
    });
    assert_eq!(text, "No matches.");
}

#[cfg(unix)]
#[test]
fn a_fifo_is_refused_instead_of_blocking() {
    let (dir, sandbox) = sandbox();
    let status = std::process::Command::new("mkfifo")
        .arg(dir.path().join("pipe"))
        .status()
        .unwrap();
    assert!(status.success());
    let ToolOutput { text, .. } = sandbox.run(&FileCall::ReadFile {
        path: "pipe".into(),
        offset: None,
    });
    assert!(text.starts_with("Error:"), "{text}");
    let ToolOutput { text, .. } = sandbox.run(&FileCall::Grep {
        pattern: "x".into(),
        path: None,
    });
    assert!(!text.starts_with("Error:"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_inside_the_folder_ends_the_search() {
    let (dir, sandbox) = sandbox();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("src/loop")).unwrap();
    let ToolOutput { text, .. } = sandbox.run(&FileCall::Grep {
        pattern: "zzz-absent".into(),
        path: None,
    });
    assert_eq!(text, "No matches.");
}
