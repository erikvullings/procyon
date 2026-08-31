//! Spotlight-backed implementation of the local indexed-search contract.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use fm_search_acceleration::{
    LocalPathReference, SearchAcceleration, SearchAccelerationCapabilities,
    SearchAccelerationError, SearchAccelerationPredicate, SearchAccelerationRequest,
    SearchAccelerationResult, SearchAccelerationScope,
};
use tokio_util::sync::CancellationToken;

/// Native Spotlight index adapter.
#[derive(Debug, Default)]
pub struct MacosSpotlightSearchAccelerator;

impl MacosSpotlightSearchAccelerator {
    /// Creates the Spotlight adapter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn spotlight_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('*', "\\*")
        .replace('?', "\\?")
}

fn spotlight_query(request: &SearchAccelerationRequest) -> String {
    let modifier = if request.case_sensitive { "" } else { "c" };
    format!(
        "kMDItemFSName == \"*{}*\"{modifier}",
        spotlight_literal(&request.name)
    )
}

fn mdfind_command(request: &SearchAccelerationRequest) -> Command {
    let mut command = Command::new("mdfind");
    command
        .arg("-onlyin")
        .arg(request.root.as_path())
        .arg(spotlight_query(request));
    command
}

fn is_finder_alias(path: &Path) -> bool {
    xattr::get(path, "com.apple.FinderInfo")
        .ok()
        .flatten()
        .is_some_and(|finder_info| finder_info.starts_with(b"alis"))
}

fn run_command(
    command: &mut Command,
    cancellation: &CancellationToken,
) -> Result<String, SearchAccelerationError> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SearchAccelerationError::Unavailable("mdfind is not installed".to_owned())
            } else {
                SearchAccelerationError::Failed(error.to_string())
            }
        })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        SearchAccelerationError::Failed("mdfind stdout was unavailable".to_owned())
    })?;
    let reader = thread::spawn(move || {
        let mut output = String::new();
        let mut stdout = stdout;
        stdout.read_to_string(&mut output).map(|_| output)
    });

    loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(SearchAccelerationError::Cancelled);
        }
        match child
            .try_wait()
            .map_err(|error| SearchAccelerationError::Failed(error.to_string()))?
        {
            Some(status) => {
                let output = reader
                    .join()
                    .map_err(|_| {
                        SearchAccelerationError::Failed("mdfind output reader panicked".to_owned())
                    })?
                    .map_err(|error| SearchAccelerationError::Failed(error.to_string()))?;
                return if status.success() {
                    Ok(output)
                } else {
                    Err(SearchAccelerationError::Unavailable(
                        "Spotlight did not accept the native query".to_owned(),
                    ))
                };
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

impl SearchAcceleration for MacosSpotlightSearchAccelerator {
    fn capabilities(&self) -> SearchAccelerationCapabilities {
        SearchAccelerationCapabilities {
            supported_predicates: vec![
                SearchAccelerationPredicate::NameSubstring,
                SearchAccelerationPredicate::CaseSensitiveNameSubstring,
            ],
            supported_scopes: vec![SearchAccelerationScope::RecursiveDirectory],
        }
    }

    fn search(
        &self,
        request: &SearchAccelerationRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchAccelerationResult>, SearchAccelerationError> {
        if cancellation.is_cancelled() {
            return Err(SearchAccelerationError::Cancelled);
        }
        let output = run_command(&mut mdfind_command(request), cancellation)?;
        output
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                let path = LocalPathReference::new(Path::new(line))?;
                Ok(SearchAccelerationResult {
                    is_alias: is_finder_alias(path.as_path()),
                    path,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(name: &str) -> SearchAccelerationRequest {
        SearchAccelerationRequest {
            root: LocalPathReference::new(Path::new("/Users/test/Documents")).unwrap(),
            name: name.to_owned(),
            case_sensitive: false,
            scope: SearchAccelerationScope::RecursiveDirectory,
        }
    }

    #[test]
    fn translates_literal_filename_queries_without_platform_syntax_leaking_to_callers() {
        assert_eq!(
            spotlight_query(&request("annual \"report\" * ? \\")),
            r#"kMDItemFSName == "*annual \"report\" \* \? \\*"c"#
        );
    }

    #[test]
    fn invokes_mdfind_with_root_and_query_as_distinct_arguments() {
        let command = mdfind_command(&request("report"));
        let arguments: Vec<_> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            arguments,
            [
                "-onlyin",
                "/Users/test/Documents",
                r#"kMDItemFSName == "*report*"c"#
            ]
        );
    }

    #[test]
    fn capabilities_cover_only_recursive_literal_filename_searches() {
        let capabilities = MacosSpotlightSearchAccelerator::new().capabilities();

        assert!(capabilities.supports(
            SearchAccelerationPredicate::NameSubstring,
            SearchAccelerationScope::RecursiveDirectory,
        ));
    }

    #[test]
    fn finder_alias_file_info_is_identified() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("alias");
        std::fs::write(&path, b"fixture").unwrap();
        let mut finder_info = [0_u8; 32];
        finder_info[..4].copy_from_slice(b"alis");
        xattr::set(&path, "com.apple.FinderInfo", &finder_info).unwrap();

        assert!(is_finder_alias(&path));
    }
}
