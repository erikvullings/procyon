//! Signs or verifies a production semantic component catalog.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use fm_semantic_components::{
    ProductionCatalogManifest, load_production_signing_key, sign_production_catalog,
    verify_production_payloads, verify_serialized_production_catalog,
    write_signed_production_catalog,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("semantic production catalog: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or("expected `sign` or `verify`")?;
    let paths: Vec<PathBuf> = arguments.map(PathBuf::from).collect();
    match (command.as_str(), paths.as_slice()) {
        ("sign", [manifest, artifacts, output]) => sign(manifest, artifacts, output),
        ("verify", [manifest, signature, artifacts, public_key]) => {
            verify(manifest, signature, artifacts, public_key)
        }
        _ => Err(
            "usage: semantic_production_catalog sign <manifest> <artifacts> <output> \
             | verify <catalog> <signature> <artifacts> <public-key>"
                .into(),
        ),
    }
}

fn sign(manifest: &Path, artifacts: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let manifest: ProductionCatalogManifest = serde_json::from_slice(&fs::read(manifest)?)?;
    verify_production_payloads(&manifest, artifacts)?;
    let signing_key = load_production_signing_key()?;
    let signed = sign_production_catalog(manifest, &signing_key)?;
    write_signed_production_catalog(&signed, output)?;
    println!("{}", output.display());
    Ok(())
}

fn verify(
    manifest_path: &Path,
    signature_path: &Path,
    artifacts: &Path,
    public_key_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let manifest_bytes = fs::read(manifest_path)?;
    let signature = fs::read(signature_path)?;
    let public_key: [u8; 32] = fs::read(public_key_path)?
        .try_into()
        .map_err(|_| "production catalog public key must contain exactly 32 bytes")?;
    let public_key = VerifyingKey::from_bytes(&public_key)?;
    let trusted = verify_serialized_production_catalog(&manifest_bytes, &signature, &public_key)?;
    let manifest: ProductionCatalogManifest = serde_json::from_slice(&manifest_bytes)?;
    verify_production_payloads(&manifest, artifacts)?;
    println!("{}", trusted.revision().as_str());
    Ok(())
}
