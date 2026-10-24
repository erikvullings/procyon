//! "Remove all Procyon data and quit" for the Settings uninstall controls.
//!
//! The running app holds log files and rewrites settings while quitting, so the deletion is
//! handed to a detached helper that waits for this process to exit first. The app bundle itself
//! is left for the OS or package manager to remove.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use fm_application::app_data::{DataBases, uninstall_paths};
use tauri::{AppHandle, Runtime};

#[cfg(unix)]
const WAIT_THEN_REMOVE: &str = r#"pid="$1"; shift
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
for path in "$@"; do rm -rf -- "$path"; done"#;

#[cfg(windows)]
const WAIT_THEN_REMOVE: &str = r#"Wait-Process -Id $env:PROCYON_REMOVE_PID -ErrorAction SilentlyContinue
foreach ($path in $env:PROCYON_REMOVE_PATHS -split '\|') {
  if ($path) { Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue }
}"#;

fn spawn_removal_helper(pid: u32, paths: &[PathBuf]) -> std::io::Result<()> {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(WAIT_THEN_REMOVE)
            .arg("procyon-remove-data")
            .arg(pid.to_string())
            .args(paths);
        command
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        // `|` cannot appear in a Windows path, so it is a safe separator.
        let joined = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("|");
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
            ])
            .arg(WAIT_THEN_REMOVE)
            .env("PROCYON_REMOVE_PID", pid.to_string())
            .env("PROCYON_REMOVE_PATHS", joined)
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

/// Schedules deletion of settings, workspaces, the semantic index, logs and caches, then quits.
#[tauri::command]
pub(crate) fn remove_application_data<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let paths = uninstall_paths(&DataBases::platform());
    spawn_removal_helper(std::process::id(), &paths).map_err(|error| error.to_string())?;
    app.exit(0);
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn helper_removes_paths_once_the_watched_process_is_gone() {
        let root = tempfile::tempdir().unwrap();
        let victim = root.path().join("procyon");
        std::fs::create_dir_all(victim.join("semantic-data")).unwrap();
        let keep = root.path().join("other");
        std::fs::create_dir_all(&keep).unwrap();
        let mut finished = Command::new("true").spawn().unwrap();
        finished.wait().unwrap();

        spawn_removal_helper(finished.id(), std::slice::from_ref(&victim)).unwrap();

        for _ in 0..50 {
            if !victim.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!victim.exists());
        assert!(keep.exists());
    }
}
