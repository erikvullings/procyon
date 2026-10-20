//! Staging of clipboard files this process is not allowed to read.
//!
//! Applications that copy files out of their own sandbox container (Microsoft Remote Desktop
//! puts files from a Windows session in `~/Library/Containers/com.microsoft.rdc.macos/...`)
//! publish plain file URLs into a directory macOS protects from other apps. Finder may read it,
//! Procyon may not. For such paths Finder is asked to copy them into a private staging directory,
//! and the staged copies are what Procyon pastes.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Staging areas older than this are removed the next time something is staged.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// Whether this process can read `path` (a file's bytes, or a directory's entries).
pub(crate) fn is_readable(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::read_dir(path).is_ok(),
        Ok(metadata) if metadata.file_type().is_symlink() => true,
        Ok(_) => std::fs::File::open(path).is_ok(),
        Err(error) => error.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

fn staging_root() -> PathBuf {
    std::env::temp_dir().join("procyon-clipboard-staging")
}

fn remove_stale(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let age = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        if age.is_some_and(|age| age > STALE_AFTER) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Escapes `value` for use inside an AppleScript string literal.
fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Has Finder copy `source` into `directory`, returning the copy.
fn stage_with_finder(source: &Path, directory: &Path) -> Option<PathBuf> {
    let source_text = source.to_str()?;
    let directory_text = directory.to_str()?;
    let name = source.file_name()?;
    let script = format!(
        "tell application \"Finder\" to duplicate (POSIX file {} as alias) to (POSIX file {} as alias) with replacing",
        applescript_string(source_text),
        applescript_string(directory_text),
    );
    let status = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?
        .status;
    let staged = directory.join(name);
    (status.success() && staged.exists()).then_some(staged)
}

/// Replaces every path this process cannot read with a Finder-made copy of it. Paths that are
/// readable, and paths Finder cannot copy either, are returned unchanged (a failed copy then
/// reports the real error when the paste runs).
pub(crate) fn stage_unreadable(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    if paths.iter().all(|path| is_readable(path)) {
        return paths;
    }
    let root = staging_root();
    remove_stale(&root);
    let session = root.join(format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos())
    ));
    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            if is_readable(&path) {
                return path;
            }
            // One directory per file, so equal names from different folders cannot collide.
            let directory = session.join(index.to_string());
            if std::fs::create_dir_all(&directory).is_err() {
                return path;
            }
            stage_with_finder(&path, &directory).unwrap_or(path)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_and_missing_paths_are_left_alone() {
        let directory = tempfile::tempdir().expect("temp directory");
        let file = directory.path().join("a.txt");
        std::fs::write(&file, b"x").expect("write");
        let missing = directory.path().join("missing.txt");
        assert!(is_readable(&file));
        assert!(is_readable(directory.path()));
        assert!(is_readable(&missing));
        let paths = vec![file, missing];
        assert_eq!(stage_unreadable(paths.clone()), paths);
    }

    #[test]
    fn applescript_strings_escape_quotes_and_backslashes() {
        assert_eq!(applescript_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
