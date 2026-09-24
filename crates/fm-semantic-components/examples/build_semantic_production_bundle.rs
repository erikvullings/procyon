//! Thin CLI over [`fm_semantic_components::build_production_release_bundle`].
//!
//! Argument order matches the Node release builder's fixed calling
//! convention exactly:
//!
//! ```text
//! build_semantic_production_bundle \
//!   <worker> <zvec-runtime> <onnx-runtime-or-dash> <model-cache> <output> \
//!   <target-os> <target-arch> <release-base-url> \
//!   <procyon-source-revision> <converter-identity> <chunker-identity>
//! ```
//!
//! Every input is explicit, including the release target, so a CI
//! invocation records exactly what it intended to build rather than
//! inferring it from the host running the job. The release version and the
//! Procyon repository URL are not arguments: both are fixed, release-wide
//! facts baked into the packer itself (see
//! [`fm_semantic_components::ProductionBundleSpec`]).

use std::ffi::OsString;
use std::path::PathBuf;

use fm_semantic_components::{
    ArtifactLocation, ProductionBundleSpec, build_production_release_bundle,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let worker_executable = PathBuf::from(required(&mut arguments, "worker executable path")?);
    let zvec_runtime_library = PathBuf::from(required(
        &mut arguments,
        "native Zvec runtime library path",
    )?);
    let onnx_runtime_library =
        optional_path(required(&mut arguments, "ONNX Runtime library path or -")?);
    let model_cache_directory =
        PathBuf::from(required(&mut arguments, "verified model cache directory")?);
    let output_directory = PathBuf::from(required(&mut arguments, "bundle output directory")?);
    let target_operating_system = required(&mut arguments, "target operating system")?;
    let target_architecture = required(&mut arguments, "target architecture")?;
    let release_base_url = ArtifactLocation::new(required(&mut arguments, "release base URL")?)?;
    let procyon_revision = required(&mut arguments, "Procyon source revision")?;
    let converter_identity = required(&mut arguments, "converter identity")?;
    let chunker_identity = required(&mut arguments, "chunker identity")?;
    if arguments.next().is_some() {
        return Err("unexpected extra argument".into());
    }

    let spec = ProductionBundleSpec {
        target_operating_system,
        target_architecture,
        procyon_revision,
        converter_identity,
        chunker_identity,
        worker_executable,
        zvec_runtime_library,
        onnx_runtime_library,
        model_cache_directory,
        release_base_url,
        output_directory: output_directory.clone(),
    };

    build_production_release_bundle(&spec)?;
    println!("{}", output_directory.display());
    Ok(())
}

fn optional_path(value: String) -> Option<PathBuf> {
    (value != "-").then(|| PathBuf::from(value))
}

fn required(
    arguments: &mut impl Iterator<Item = OsString>,
    label: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    arguments
        .next()
        .map(|value| value.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{label} is required").into())
}
