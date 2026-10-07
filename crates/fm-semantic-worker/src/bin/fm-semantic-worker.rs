//! Isolated local semantic worker process.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), fm_semantic_worker::ServerError> {
    let arguments = arguments_from(std::env::args_os().skip(1))?;
    #[cfg(feature = "semantic-runtime")]
    if let Some(semantic_data_directory) = arguments.semantic_data_directory {
        if arguments.development_mode {
            return fm_semantic_worker::developer_bundle::run_developer_worker(
                &arguments.runtime_directory,
                &semantic_data_directory,
                arguments.semantic_model_pack.as_deref(),
                arguments.idle_timeout,
            )
            .await;
        }
        let model = arguments.managed_model.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "one managed model is required with --semantic-data-dir",
            )
        })?;
        return fm_semantic_worker::developer_bundle::run_managed_worker(
            &arguments.runtime_directory,
            &semantic_data_directory,
            &model,
            arguments.ocrmypdf_executable.as_deref(),
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
    #[cfg(feature = "semantic-runtime")]
    semantic_data_directory: Option<PathBuf>,
    #[cfg(feature = "semantic-runtime")]
    semantic_model_pack: Option<PathBuf>,
    #[cfg(feature = "semantic-runtime")]
    managed_model: Option<fm_semantic_worker::ManagedModel>,
    #[cfg(feature = "semantic-runtime")]
    ocrmypdf_executable: Option<PathBuf>,
    #[cfg(feature = "semantic-runtime")]
    development_mode: bool,
}

fn arguments_from(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Arguments, io::Error> {
    let mut arguments = arguments.into_iter();
    let mut runtime_directory = None;
    let mut idle_timeout = Duration::from_secs(30);
    #[cfg(feature = "semantic-runtime")]
    let mut semantic_data_directory = None;
    #[cfg(feature = "semantic-runtime")]
    let mut semantic_model_pack = None;
    #[cfg(feature = "gemma-native")]
    let mut gemma_paths = [None, None, None, None, None];
    #[cfg(feature = "gemma-native")]
    let mut gemma_dimensions = None;
    #[cfg(feature = "gemma-native")]
    let mut gemma_media = fm_semantic_worker::gemma_native::GemmaMedia {
        images: false,
        audio: false,
        video: false,
    };
    #[cfg(feature = "gemma-native")]
    let mut gemma_selected = false;
    #[cfg(feature = "semantic-runtime")]
    let mut ocrmypdf_executable = None;
    #[cfg(all(feature = "semantic-runtime", feature = "developer-bundle"))]
    let mut development_mode = false;
    #[cfg(all(feature = "semantic-runtime", not(feature = "developer-bundle")))]
    let development_mode = false;
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
        } else if argument == "--semantic-data-dir" {
            #[cfg(feature = "semantic-runtime")]
            {
                semantic_data_directory =
                    Some(PathBuf::from(arguments.next().ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "semantic data directory value is required",
                        )
                    })?));
            }
            #[cfg(not(feature = "semantic-runtime"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--semantic-data-dir requires the opt-in semantic-runtime feature",
                ));
            }
        } else if argument == "--semantic-model-pack" {
            #[cfg(feature = "semantic-runtime")]
            {
                semantic_model_pack = Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "semantic model pack value is required",
                    )
                })?));
            }
            #[cfg(not(feature = "semantic-runtime"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--semantic-model-pack requires the opt-in semantic-runtime feature",
                ));
            }
        } else if argument == "--ocrmypdf-executable" {
            #[cfg(feature = "semantic-runtime")]
            {
                ocrmypdf_executable = Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "ocrmypdf executable value is required",
                    )
                })?));
            }
            #[cfg(not(feature = "semantic-runtime"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--ocrmypdf-executable requires the opt-in semantic-runtime feature",
                ));
            }
        } else if argument.to_string_lossy().starts_with("--gemma-") {
            #[cfg(feature = "gemma-native")]
            {
                let flag = argument.to_string_lossy();
                let position = match flag.as_ref() {
                    "--gemma-weights" => Some(0),
                    "--gemma-tokenizer" => Some(1),
                    "--gemma-config" => Some(2),
                    "--gemma-visual-processor" => Some(3),
                    "--gemma-audio-processor" => Some(4),
                    _ => None,
                };
                gemma_selected = true;
                if let Some(position) = position {
                    if gemma_paths[position].is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "duplicate Gemma file",
                        ));
                    }
                    gemma_paths[position] =
                        Some(PathBuf::from(arguments.next().ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidInput, "Gemma file path required")
                        })?));
                } else if flag == "--gemma-dimensions" {
                    if gemma_dimensions.is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "duplicate Gemma dimensions",
                        ));
                    }
                    gemma_dimensions = Some(
                        arguments
                            .next()
                            .ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "Gemma dimensions required",
                                )
                            })?
                            .to_string_lossy()
                            .parse::<usize>()
                            .map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "invalid Gemma dimensions",
                                )
                            })?,
                    );
                } else {
                    let enabled = match flag.as_ref() {
                        "--gemma-images" => &mut gemma_media.images,
                        "--gemma-audio" => &mut gemma_media.audio,
                        "--gemma-video" => &mut gemma_media.video,
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "unknown Gemma argument",
                            ));
                        }
                    };
                    if *enabled {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "duplicate Gemma media flag",
                        ));
                    }
                    *enabled = true;
                }
            }
            #[cfg(not(feature = "gemma-native"))]
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Gemma arguments require the gemma-native feature",
            ));
        } else if argument == "--developer-data-dir" {
            #[cfg(feature = "developer-bundle")]
            {
                development_mode = true;
                semantic_data_directory =
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
        } else if argument == "--developer-model-pack" {
            #[cfg(feature = "developer-bundle")]
            {
                development_mode = true;
                semantic_model_pack = Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "developer model pack value is required",
                    )
                })?));
            }
            #[cfg(not(feature = "developer-bundle"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--developer-model-pack requires the opt-in developer-bundle feature",
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
    #[cfg(feature = "semantic-runtime")]
    let managed_model = {
        #[cfg(feature = "gemma-native")]
        if gemma_selected {
            if semantic_model_pack.is_some()
                || development_mode
                || semantic_data_directory.is_none()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Gemma requires managed data and excludes a model pack",
                ));
            }
            let [
                weights,
                tokenizer,
                config,
                visual_processor,
                audio_processor,
            ] = gemma_paths.map(|path| {
                path.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "all five Gemma files are required",
                    )
                })
            });
            let dimensions = gemma_dimensions.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Gemma dimensions are required")
            })?;
            if ![128, 256, 512, 768].contains(&dimensions) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsupported Gemma dimensions",
                ));
            }
            Some(fm_semantic_worker::ManagedModel::Gemma {
                files: fm_semantic_worker::gemma_native::GemmaNativeFiles {
                    weights: weights?,
                    tokenizer: tokenizer?,
                    config: config?,
                    visual_processor: visual_processor?,
                    audio_processor: audio_processor?,
                },
                dimensions,
                media: gemma_media,
            })
        } else {
            semantic_model_pack
                .clone()
                .map(fm_semantic_worker::ManagedModel::Pack)
        }
        #[cfg(not(feature = "gemma-native"))]
        {
            semantic_model_pack
                .clone()
                .map(fm_semantic_worker::ManagedModel::Pack)
        }
    };
    Ok(Arguments {
        runtime_directory,
        idle_timeout,
        #[cfg(feature = "semantic-runtime")]
        semantic_data_directory,
        #[cfg(feature = "semantic-runtime")]
        semantic_model_pack,
        #[cfg(feature = "semantic-runtime")]
        managed_model,
        #[cfg(feature = "semantic-runtime")]
        ocrmypdf_executable,
        #[cfg(feature = "semantic-runtime")]
        development_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "semantic-runtime"))]
    #[test]
    fn semantic_arguments_require_the_feature() {
        for argument in ["--semantic-data-dir", "--semantic-model-pack"] {
            let error = arguments_from([
                "--runtime-dir".into(),
                "runtime".into(),
                argument.into(),
                "value".into(),
            ])
            .err()
            .expect("argument must be rejected");

            assert!(error.to_string().contains("semantic-runtime feature"));
        }
    }

    #[cfg(all(feature = "semantic-runtime", not(feature = "developer-bundle")))]
    #[test]
    fn managed_data_and_model_arguments_are_explicitly_parsed() {
        let arguments = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--semantic-data-dir".into(),
            "data".into(),
            "--semantic-model-pack".into(),
            "pack".into(),
        ])
        .expect("arguments");

        assert_eq!(
            arguments.semantic_data_directory,
            Some(PathBuf::from("data"))
        );
        assert_eq!(arguments.semantic_model_pack, Some(PathBuf::from("pack")));
        assert!(arguments.ocrmypdf_executable.is_none());
        assert!(!arguments.development_mode);
    }

    #[cfg(feature = "semantic-runtime")]
    #[test]
    fn managed_ocrmypdf_executable_argument_is_explicitly_parsed() {
        let arguments = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--semantic-data-dir".into(),
            "data".into(),
            "--semantic-model-pack".into(),
            "pack".into(),
            "--ocrmypdf-executable".into(),
            "/usr/local/bin/ocrmypdf".into(),
        ])
        .expect("arguments");

        assert_eq!(
            arguments.ocrmypdf_executable,
            Some(PathBuf::from("/usr/local/bin/ocrmypdf"))
        );
    }

    #[cfg(not(feature = "semantic-runtime"))]
    #[test]
    fn ocrmypdf_argument_requires_the_semantic_runtime_feature() {
        let error = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--ocrmypdf-executable".into(),
            "/usr/local/bin/ocrmypdf".into(),
        ])
        .err()
        .expect("argument must be rejected");

        assert!(error.to_string().contains("semantic-runtime feature"));
    }

    #[cfg(feature = "developer-bundle")]
    #[test]
    fn developer_data_and_model_arguments_are_explicitly_parsed() {
        let arguments = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--developer-data-dir".into(),
            "data".into(),
            "--developer-model-pack".into(),
            "pack".into(),
        ])
        .expect("arguments");

        assert_eq!(
            arguments.semantic_data_directory,
            Some(PathBuf::from("data"))
        );
        assert_eq!(arguments.semantic_model_pack, Some(PathBuf::from("pack")));
        assert!(arguments.development_mode);
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        let error = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--load-model-from".into(),
            "https://example.invalid/model".into(),
        ])
        .err()
        .expect("unknown argument must be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(feature = "gemma-native")]
    #[test]
    fn gemma_cli_requires_complete_original_files_and_excludes_pack() {
        let base = ["--runtime-dir", "runtime", "--semantic-data-dir", "data"];
        let files = [
            "--gemma-weights",
            "/model/weights",
            "--gemma-tokenizer",
            "/model/tokenizer",
            "--gemma-config",
            "/model/config",
            "--gemma-visual-processor",
            "/model/visual",
            "--gemma-audio-processor",
            "/model/audio",
            "--gemma-dimensions",
            "512",
            "--gemma-images",
            "--gemma-video",
        ];
        let complete = arguments_from(base.into_iter().chain(files).map(Into::into)).unwrap();
        assert!(matches!(
            complete.managed_model,
            Some(fm_semantic_worker::ManagedModel::Gemma {
                dimensions: 512,
                ..
            })
        ));
        assert!(
            arguments_from(
                base.into_iter()
                    .chain(files[..8].iter().copied())
                    .chain(files[10..].iter().copied())
                    .map(Into::into)
            )
            .is_err()
        );
        assert!(
            arguments_from(
                base.into_iter()
                    .chain(files)
                    .chain(["--semantic-model-pack", "/model/pack"])
                    .map(Into::into)
            )
            .is_err()
        );
        let mut unsupported = files;
        unsupported[11] = "999";
        assert!(arguments_from(base.into_iter().chain(unsupported).map(Into::into)).is_err());
        assert!(
            arguments_from(
                base.into_iter()
                    .chain(files)
                    .chain(["--gemma-audio", "--gemma-audio"])
                    .map(Into::into)
            )
            .is_err()
        );
    }

    #[cfg(not(feature = "gemma-native"))]
    #[test]
    fn gemma_cli_fails_closed_when_native_feature_is_absent() {
        let error = arguments_from([
            "--runtime-dir".into(),
            "runtime".into(),
            "--gemma-weights".into(),
            "/model/weights".into(),
        ])
        .err()
        .unwrap();
        assert!(error.to_string().contains("gemma-native feature"));
    }
}
