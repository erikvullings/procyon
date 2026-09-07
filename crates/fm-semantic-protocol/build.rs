//! Generates the semantic-worker protobuf DTOs with a vendored compiler.

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config.compile_protos(&["proto/semantic/v1/worker.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/semantic/v1/worker.proto");
    Ok(())
}
