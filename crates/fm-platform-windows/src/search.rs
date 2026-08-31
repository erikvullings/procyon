//! Windows Search-backed implementation of the local indexed-search contract.

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

/// Fixed Windows Search script. Root and filename values are passed separately
/// as PowerShell arguments; the script escapes their SQL contexts before the
/// `Search.CollatorDSO` query is created.
const WINDOWS_SEARCH_SCRIPT: &str = r#"
param([string]$Root, [string]$Name)
function Escape-Like([string]$Value) {
  return $Value.Replace("'", "''").Replace("[", "[[]").Replace("%", "[%]").Replace("_", "[_]")
}
$connection = New-Object -ComObject ADODB.Connection
$connection.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows';")
$scope = "file:" + $Root.Replace("\", "/").TrimEnd("/") + "/"
$escapedScope = $scope.Replace("'", "''")
$escapedName = Escape-Like $Name
$command = New-Object -ComObject ADODB.Command
$command.ActiveConnection = $connection
$command.CommandText = "SELECT System.ItemPathDisplay FROM SYSTEMINDEX WHERE SCOPE = '" + $escapedScope + "' AND System.FileName LIKE '%" + $escapedName + "%'"
$recordset = $command.Execute()
while (!$recordset.EOF) {
  [Console]::Out.WriteLine(($recordset.Fields.Item("System.ItemPathDisplay").Value | ConvertTo-Json -Compress))
  $recordset.MoveNext()
}
$recordset.Close()
$connection.Close()
"#;

/// Native Windows Search adapter.
#[derive(Debug, Default)]
pub struct WindowsSearchAccelerator;

impl WindowsSearchAccelerator {
    /// Creates the Windows Search adapter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

fn powershell_command(request: &SearchAccelerationRequest) -> Command {
    let mut command = Command::new("powershell.exe");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(WINDOWS_SEARCH_SCRIPT)
        .arg("-Root")
        .arg(request.root.as_path())
        .arg("-Name")
        .arg(&request.name);
    command
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
                SearchAccelerationError::Unavailable("PowerShell is not installed".to_owned())
            } else {
                SearchAccelerationError::Failed(error.to_string())
            }
        })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        SearchAccelerationError::Failed("Windows Search stdout was unavailable".to_owned())
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
                        SearchAccelerationError::Failed(
                            "Windows Search output reader panicked".to_owned(),
                        )
                    })?
                    .map_err(|error| SearchAccelerationError::Failed(error.to_string()))?;
                return if status.success() {
                    Ok(output)
                } else {
                    Err(SearchAccelerationError::Unavailable(
                        "Windows Search did not accept the native query".to_owned(),
                    ))
                };
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

impl SearchAcceleration for WindowsSearchAccelerator {
    fn capabilities(&self) -> SearchAccelerationCapabilities {
        SearchAccelerationCapabilities {
            supported_predicates: vec![SearchAccelerationPredicate::NameSubstring],
            supported_scopes: vec![SearchAccelerationScope::RecursiveDirectory],
        }
    }

    fn search(
        &self,
        request: &SearchAccelerationRequest,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchAccelerationResult>, SearchAccelerationError> {
        if request.case_sensitive
            || request.scope != SearchAccelerationScope::RecursiveDirectory
            || cancellation.is_cancelled()
        {
            return Err(if cancellation.is_cancelled() {
                SearchAccelerationError::Cancelled
            } else {
                SearchAccelerationError::Unsupported
            });
        }

        let output = run_command(&mut powershell_command(request), cancellation)?;
        output
            .lines()
            .map(|line| {
                let path: String = serde_json::from_str(line).map_err(|error| {
                    SearchAccelerationError::Failed(format!(
                        "Windows Search returned an invalid path record: {error}"
                    ))
                })?;
                Ok(SearchAccelerationResult {
                    path: LocalPathReference::new(Path::new(&path))?,
                    is_alias: false,
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
            root: LocalPathReference::new(Path::new(r"C:\Users\test\Documents")).unwrap(),
            name: name.to_owned(),
            case_sensitive: false,
            scope: SearchAccelerationScope::RecursiveDirectory,
        }
    }

    #[test]
    fn invokes_windows_search_with_user_values_as_separate_arguments() {
        let command = powershell_command(&request("report'; Remove-Item *"));
        let arguments: Vec<_> = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();

        assert!(arguments.contains(&WINDOWS_SEARCH_SCRIPT.to_owned()));
        assert_eq!(
            arguments[arguments.len() - 4..],
            [
                "-Root",
                r"C:\Users\test\Documents",
                "-Name",
                "report'; Remove-Item *"
            ]
        );
    }

    #[test]
    fn only_advertises_case_insensitive_recursive_filename_search() {
        let capabilities = WindowsSearchAccelerator::new().capabilities();

        assert!(capabilities.supports(
            SearchAccelerationPredicate::NameSubstring,
            SearchAccelerationScope::RecursiveDirectory,
        ));
        assert!(
            !capabilities
                .supported_predicates
                .contains(&SearchAccelerationPredicate::CaseSensitiveNameSubstring)
        );
    }
}
