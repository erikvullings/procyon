//! Isolated local semantic worker process.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), fm_semantic_worker::ServerError> {
    let arguments = arguments_from(std::env::args_os().skip(1))?;
    #[cfg(feature = "developer-bundle")]
    if let Some(developer_data_directory) = arguments.developer_data_directory {
        return fm_semantic_worker::developer_bundle::run_developer_worker(
            &arguments.runtime_directory,
            &developer_data_directory,
            arguments.idle_timeout,
        )
        .await;
    }
    fm_semantic_worker::run_desktop_worker(&arguments.runtime_directory, arguments.idle_timeout)
        .await
}

struct Arguments {
    runtime_directory: PathBuf,
    idle_timeout: Duration,
    #[cfg(feature = "developer-bundle")]
    developer_data_directory: Option<PathBuf>,
}

fn arguments_from(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Arguments, io::Error> {
    let mut arguments = arguments.into_iter();
    let mut runtime_directory = None;
    let mut idle_timeout = Duration::from_secs(30);
    #[cfg(feature = "developer-bundle")]
    let mut developer_data_directory = None;
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
        } else if argument == "--developer-data-dir" {
            #[cfg(feature = "developer-bundle")]
            {
                developer_data_directory =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "developer data directory value is required",
                        )
                    })?));
            }
            #[cfg(not(feature = "developer-bundle"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--developer-data-dir requires the opt-in developer-bundle feature",
                ));
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown semantic worker argument",
            ));
        }
    }
    let runtime_directory = runtime_directory
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "--runtime-dir is required"))?;
    Ok(Arguments {
        runtime_directory,
        idle_timeout,
        #[cfg(feature = "developer-bundle")]
        developer_data_directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "developer-bundle"))]
    #[test]
    fn developer_data_argument_requires_the_feature() {
        let error = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--developer-data-dir".into(),
            "data".into(),
        ])
        .err()
        .expect("argument must be rejected");

        assert!(error.to_string().contains("developer-bundle feature"));
    }

    #[cfg(feature = "developer-bundle")]
    #[test]
    fn developer_data_argument_is_explicitly_parsed() {
        let arguments = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--developer-data-dir".into(),
            "data".into(),
        ])
        .expect("arguments");

        assert_eq!(
            arguments.developer_data_directory,
            Some(PathBuf::from("data"))
        );
    }
}
