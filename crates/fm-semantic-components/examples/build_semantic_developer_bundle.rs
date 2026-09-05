//! Builds a host-platform semantic bundle for local pipeline testing.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use fm_semantic_components::{
    ArtifactCompatibility, ArtifactId, ArtifactKind, ArtifactLocation, CatalogArtifact,
    CatalogManifest, ComponentId, ComponentResources, EmbeddingNormalization, LicenseInfo,
    ManifestRevision, ModelId, ModelIdentity, ModelManifest, ModelMetadata, ModelRevision,
    ProtocolRange, RuntimeCompatibility, SemanticProfile, Sha256Digest, TargetTriple, TokenizerId,
};
use semver::{Version, VersionReq};

const DEVELOPMENT_SIGNING_KEY: [u8; 32] = [0x19; 32];
const DEVELOPMENT_MODEL_DIMENSIONS: u32 = 384;
const INDEX_SCHEMA_VERSION: u32 = 1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let worker = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("worker executable path is required")?;
    let native_runtime = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("Zvec native runtime path is required")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("bundle output directory is required")?;
    if arguments.next().is_some() {
        return Err(
            "expected exactly a worker path, Zvec runtime path, and output directory".into(),
        );
    }
    if !worker.is_file() {
        return Err(format!("semantic worker does not exist: {}", worker.display()).into());
    }
    if !native_runtime.is_file() {
        return Err(format!(
            "Zvec native runtime does not exist: {}",
            native_runtime.display()
        )
        .into());
    }

    build_bundle(&worker, &native_runtime, &output)?;
    println!("{}", output.display());
    Ok(())
}

fn build_bundle(
    worker: &Path,
    native_runtime: &Path,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = TargetTriple::new(std::env::consts::OS, std::env::consts::ARCH)?;
    let version = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let target_label = format!("{}-{}", target.operating_system(), target.architecture());
    let worker_id = ArtifactId::new(format!(
        "procyon.dev.worker.{target_label}.{}",
        version.to_string().replace('-', ".")
    ))?;
    let runtime_id = ArtifactId::new(format!(
        "procyon.dev.runtime.{target_label}.{}",
        version.to_string().replace('-', ".")
    ))?;
    let model_id = ArtifactId::new("procyon.dev.model.hashing-embedding.v1")?;

    let staging = output.with_extension("building");
    remove_existing(&staging)?;
    fs::create_dir_all(staging.join("artifacts"))?;

    let worker_bytes = fs::read(worker)?;
    write_artifact(&staging, &worker_id, &worker_bytes)?;
    preserve_executable_permissions(worker, &staging.join("artifacts").join(worker_id.as_str()))?;

    let runtime_bytes = fs::read(native_runtime)?;
    write_artifact(&staging, &runtime_id, &runtime_bytes)?;

    let model_bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "kind": "deterministic-token-hashing-embedding",
        "revision": "sha256-token-hashing-v1",
        "tokenizer": "unicode-words-v1",
        "dimensions": DEVELOPMENT_MODEL_DIMENSIONS,
        "production": false,
        "warning": "Pipeline validation only; this is not an evaluated semantic model."
    }))?;
    write_artifact(&staging, &model_id, &model_bytes)?;

    let runtime_component = ComponentId::new("procyon.dev.runtime")?;
    let model_identity = ModelIdentity::new(
        ModelId::new("procyon.dev.hashing-embedding")?,
        ModelRevision::new("sha256-token-hashing-v1")?,
    );
    let model_metadata = ModelMetadata::new(
        model_identity.clone(),
        LicenseInfo::new(
            "MIT",
            "Procyon deterministic development fixture; not a trained model.",
        )?,
        TokenizerId::new("unicode-words-v1")?,
        DEVELOPMENT_MODEL_DIMENSIONS,
        EmbeddingNormalization::UnitLength,
        RuntimeCompatibility::new(
            runtime_component.clone(),
            VersionReq::parse(&format!("={version}"))?,
        ),
        ["development", "language-agnostic-token-overlap"],
        u64::try_from(model_bytes.len())?,
        8 * 1024 * 1024,
    )?;

    let worker_artifact = CatalogArtifact::new(
        worker_id.clone(),
        ComponentId::new("procyon.dev.worker")?,
        ArtifactKind::Worker,
        version.clone(),
        development_location(&worker_id)?,
        LicenseInfo::new("MIT", "Procyon semantic worker development build.")?,
        Sha256Digest::calculate(&worker_bytes),
        resources(&worker_bytes, 64 * 1024 * 1024)?,
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 1)?),
            Vec::new(),
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let runtime_artifact = CatalogArtifact::new(
        runtime_id.clone(),
        runtime_component,
        ArtifactKind::Runtime,
        version,
        development_location(&runtime_id)?,
        LicenseInfo::new(
            "Apache-2.0",
            "Zvec native runtime for the explicitly non-production developer bundle.",
        )?,
        Sha256Digest::calculate(&runtime_bytes),
        resources(&runtime_bytes, 8 * 1024 * 1024)?,
        ArtifactCompatibility::new(Some(target), None, Vec::new(), INDEX_SCHEMA_VERSION),
    )?;
    let model_artifact = CatalogArtifact::new(
        model_id.clone(),
        ComponentId::new("procyon.dev.model.hashing-embedding")?,
        ArtifactKind::Model(model_identity),
        Version::new(1, 0, 0),
        development_location(&model_id)?,
        model_metadata.license().clone(),
        Sha256Digest::calculate(&model_bytes),
        resources(&model_bytes, model_metadata.estimated_ram_bytes())?,
        ArtifactCompatibility::new(
            None,
            None,
            vec![model_metadata.runtime().clone()],
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let model_manifest = ModelManifest::new(model_id, model_metadata);
    let profiles = SemanticProfile::all()
        .iter()
        .copied()
        .map(|profile| (profile, model_manifest.metadata().identity().clone()))
        .collect::<BTreeMap<_, _>>();
    let manifest = CatalogManifest::new(
        ManifestRevision::new(format!(
            "procyon-dev-{target_label}-{}",
            env!("CARGO_PKG_VERSION")
        ))?,
        vec![worker_artifact, runtime_artifact, model_artifact],
        vec![model_manifest],
        profiles,
    )?;
    let signature =
        SigningKey::from_bytes(&DEVELOPMENT_SIGNING_KEY).sign(&manifest.canonical_bytes()?);
    fs::write(
        staging.join("catalog.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::write(staging.join("catalog.sig"), signature.to_bytes())?;
    fs::write(
        staging.join("DEVELOPMENT-ONLY.txt"),
        b"This bundle is signed by a public repository development key.\n\
It validates local packaging and end-to-end semantic plumbing only.\n\
It is not an evaluated or supported production semantic component pack.\n",
    )?;

    remove_existing(output)?;
    fs::rename(staging, output)?;
    Ok(())
}

fn development_location(id: &ArtifactId) -> Result<ArtifactLocation, Box<dyn std::error::Error>> {
    Ok(ArtifactLocation::new(format!(
        "https://developer.invalid/artifacts/{}",
        id.as_str()
    ))?)
}

fn resources(
    bytes: &[u8],
    ram_bytes: u64,
) -> Result<ComponentResources, Box<dyn std::error::Error>> {
    let size = u64::try_from(bytes.len())?;
    Ok(ComponentResources::new(size, size, ram_bytes)?)
}

fn write_artifact(root: &Path, id: &ArtifactId, bytes: &[u8]) -> io::Result<()> {
    fs::write(root.join("artifacts").join(id.as_str()), bytes)
}

fn remove_existing(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn preserve_executable_permissions(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_semantic_components::{SignedCatalogManifest, TrustedCatalog};

    #[test]
    fn builds_a_complete_verifiable_development_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("fm-semantic-worker");
        let runtime = directory.path().join("libzvec_c_api.fixture");
        let output = directory.path().join("bundle");
        fs::write(&worker, b"development worker fixture").unwrap();
        fs::write(&runtime, b"development Zvec fixture").unwrap();
        build_bundle(&worker, &runtime, &output).unwrap();

        let manifest: CatalogManifest =
            serde_json::from_slice(&fs::read(output.join("catalog.json")).unwrap()).unwrap();
        let signature: [u8; 64] = fs::read(output.join("catalog.sig"))
            .unwrap()
            .try_into()
            .unwrap();
        let signing_key = SigningKey::from_bytes(&DEVELOPMENT_SIGNING_KEY);
        let trusted = TrustedCatalog::verify(
            SignedCatalogManifest::new(manifest, signature),
            &signing_key.verifying_key(),
        )
        .unwrap();

        assert_eq!(trusted.artifacts().len(), 3);
        for artifact in trusted.artifacts() {
            let payload = fs::read(output.join("artifacts").join(artifact.id().as_str())).unwrap();
            assert_eq!(Sha256Digest::calculate(&payload), artifact.checksum());
        }
        assert!(
            fs::read_to_string(output.join("DEVELOPMENT-ONLY.txt"))
                .unwrap()
                .contains("not an evaluated or supported production")
        );
    }

    #[test]
    fn replaces_stale_bundle_contents_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("fm-semantic-worker");
        let runtime = directory.path().join("libzvec_c_api.fixture");
        let output = directory.path().join("bundle");
        fs::write(&worker, b"first worker").unwrap();
        fs::write(&runtime, b"development Zvec fixture").unwrap();
        build_bundle(&worker, &runtime, &output).unwrap();
        fs::write(output.join("stale"), b"stale").unwrap();

        fs::write(&worker, b"second worker").unwrap();
        build_bundle(&worker, &runtime, &output).unwrap();

        assert!(!output.join("stale").exists());
        let manifest: CatalogManifest =
            serde_json::from_slice(&fs::read(output.join("catalog.json")).unwrap()).unwrap();
        let signature: [u8; 64] = fs::read(output.join("catalog.sig"))
            .unwrap()
            .try_into()
            .unwrap();
        let signing_key = SigningKey::from_bytes(&DEVELOPMENT_SIGNING_KEY);
        let trusted = TrustedCatalog::verify(
            SignedCatalogManifest::new(manifest, signature),
            &signing_key.verifying_key(),
        )
        .unwrap();
        let worker_artifact = trusted
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .unwrap();
        assert_eq!(
            worker_artifact.checksum(),
            Sha256Digest::calculate(b"second worker")
        );
    }
}
