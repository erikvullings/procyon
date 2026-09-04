//! Isolated local semantic worker process.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), fm_semantic_worker::ServerError> {
    let (runtime_directory, idle_timeout) = arguments()?;
    fm_semantic_worker::run_desktop_worker(&runtime_directory, idle_timeout).await
}

fn arguments() -> Result<(PathBuf, Duration), io::Error> {
    let mut arguments = std::env::args_os().skip(1);
    let mut runtime_directory = None;
    let mut idle_timeout = Duration::from_secs(30);
    while let Some(argument) = arguments.next() {
        if argument == "--runtime-dir" {
            runtime_directory = arguments.next().map(PathBuf::from);
        } else if argument == "--idle-timeout-ms" {
            let value = arguments.next().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "idle timeout value is required",
                )
            })?;
            let value = value.to_string_lossy().parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "idle timeout must be milliseconds",
                )
            })?;
            idle_timeout = Duration::from_millis(value);
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown semantic worker argument",
            ));
        }
    }
    let runtime_directory = runtime_directory
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--runtime-dir is required"))?;
    Ok((runtime_directory, idle_timeout))
}
