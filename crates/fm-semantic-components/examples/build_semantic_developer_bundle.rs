//! Builds a host-platform semantic bundle for local pipeline testing.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use fm_semantic_components::{
    ArtifactCompatibility, ArtifactId, ArtifactKind, ArtifactLocation, CatalogArtifact,
    CatalogManifest, ComponentId, ComponentResources, EmbeddingNormalization, LicenseInfo,
    ManifestRevision, ModelId, ModelIdentity, ModelManifest, ModelMetadata, ModelPack,
    ModelPackKind, ModelPackSpec, ModelRevision, ProtocolRange, RuntimeCompatibility,
    SemanticProfile, Sha256Digest, TargetTriple, TokenizerId, write_model_pack,
};
use semver::{Version, VersionReq};

const DEVELOPMENT_SIGNING_KEY: [u8; 32] = [0x19; 32];
const DEVELOPMENT_MODEL_DIMENSIONS: u32 = 384;
const INDEX_SCHEMA_VERSION: u32 = 2;

/// Immutable upstream identity of the real multilingual retrieval model.
///
/// Every value here must stay in lockstep with `scripts/fetch-semantic-model.mjs`.
/// The script verifies downloads and this builder independently verifies the
/// cache immediately before signing it into the catalog.
const MULTILINGUAL_REPOSITORY: &str = "intfloat/multilingual-e5-small";
const MULTILINGUAL_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
const MULTILINGUAL_MODEL_ID: &str = "intfloat.multilingual-e5-small";
const MULTILINGUAL_TOKENIZER: &str = "xlm-roberta-sentencepiece.614241f6";
const MULTILINGUAL_DIMENSIONS: u32 = 384;
const MULTILINGUAL_MAX_INPUT_TOKENS: u32 = 512;
const MULTILINGUAL_QUERY_PREFIX: &str = "query: ";
const MULTILINGUAL_PASSAGE_PREFIX: &str = "passage: ";
/// Conservative peak resident bytes while loading and running the graph.
///
/// This is the single figure the catalog declares and the documentation quotes:
/// 1,600 MiB, rounded up from the observed peak so the free-space and memory
/// disclosures never understate what selecting the profile costs.
const MULTILINGUAL_RAM_BYTES: u64 = 1_600 * 1024 * 1024;
const MULTILINGUAL_FILES: [PinnedModelFile; 5] = [
    PinnedModelFile {
        name: "model.onnx",
        bytes: 470_268_510,
        sha256: "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665",
    },
    PinnedModelFile {
        name: "tokenizer.json",
        bytes: 17_082_730,
        sha256: "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    },
    PinnedModelFile {
        name: "config.json",
        bytes: 655,
        sha256: "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
    },
    PinnedModelFile {
        name: "tokenizer_config.json",
        bytes: 443,
        sha256: "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b",
    },
    PinnedModelFile {
        name: "special_tokens_map.json",
        bytes: 167,
        sha256: "d05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7",
    },
];

#[derive(Clone, Copy)]
struct PinnedModelFile {
    name: &'static str,
    bytes: u64,
    sha256: &'static str,
}

struct VerifiedModelCache(PathBuf);

impl VerifiedModelCache {
    fn path(&self) -> &Path {
        &self.0
    }
}
/// Languages the upstream model card lists first; the pack covers many more.
const MULTILINGUAL_LANGUAGES: [&str; 12] = [
    "ar", "de", "en", "es", "fr", "hi", "it", "ja", "nl", "pt", "ru", "zh",
];

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
    let model_cache = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("verified model cache directory is required")?;
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("bundle output directory is required")?;
    if arguments.next().is_some() {
        return Err("expected a worker path, Zvec runtime path, model cache directory, and output directory".into());
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
    let model_cache = verify_multilingual_cache(&model_cache)?;
    build_bundle(&worker, &native_runtime, &model_cache, &output)?;
    println!("{}", output.display());
    Ok(())
}

fn build_bundle(
    worker: &Path,
    native_runtime: &Path,
    model_cache: &VerifiedModelCache,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    build_bundle_inner(worker, native_runtime, model_cache, output, true)
}

fn build_bundle_inner(
    worker: &Path,
    native_runtime: &Path,
    model_cache: &VerifiedModelCache,
    output: &Path,
    verify_pinned_pack: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = TargetTriple::new(std::env::consts::OS, std::env::consts::ARCH)?;
    let version = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let target_label = format!("{}-{}", target.operating_system(), target.architecture());

    let staging = output.with_extension("building");
    remove_existing(&staging)?;
    fs::create_dir_all(staging.join("artifacts"))?;

    let worker_bytes = fs::read(worker)?;
    let worker_checksum = Sha256Digest::calculate(&worker_bytes);
    let worker_id = content_addressed_id(
        &format!(
            "procyon.dev.worker.{target_label}.{}",
            version.to_string().replace('-', ".")
        ),
        worker_checksum,
    )?;
    write_artifact(&staging, &worker_id, &worker_bytes)?;
    preserve_executable_permissions(worker, &staging.join("artifacts").join(worker_id.as_str()))?;

    let runtime_bytes = fs::read(native_runtime)?;
    let runtime_checksum = Sha256Digest::calculate(&runtime_bytes);
    let runtime_id = content_addressed_id(
        &format!(
            "procyon.dev.runtime.{target_label}.{}",
            version.to_string().replace('-', ".")
        ),
        runtime_checksum,
    )?;
    write_artifact(&staging, &runtime_id, &runtime_bytes)?;

    // The deterministic fixture carries no learned parameters, so its pack is
    // an index and nothing else. Packing it anyway keeps one loading path in
    // the worker and one activation contract in the host.
    let fixture_pack = staging.join("artifacts").join(".hashing-model-pack");
    write_model_pack(
        &fixture_pack,
        &ModelPackSpec {
            kind: ModelPackKind::DeterministicTokenHashing,
            model_id: "procyon.dev.hashing-embedding".into(),
            model_revision: "sha256-token-hashing-v1".into(),
            tokenizer: "unicode-words-v1".into(),
            dimensions: DEVELOPMENT_MODEL_DIMENSIONS,
            max_input_tokens: 8_192,
            query_prefix: String::new(),
            passage_prefix: String::new(),
            production: false,
            source: "Procyon deterministic token-hashing fixture".into(),
            files: Vec::new(),
        },
    )?;
    let model_bytes = fs::read(&fixture_pack)?;
    let model_checksum = Sha256Digest::calculate(&model_bytes);
    let model_id = content_addressed_id("procyon.dev.model.hashing-embedding.v1", model_checksum)?;
    fs::rename(
        &fixture_pack,
        staging.join("artifacts").join(model_id.as_str()),
    )?;

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

    let multilingual_pack = staging.join("artifacts").join(".multilingual-model-pack");
    write_model_pack(
        &multilingual_pack,
        &ModelPackSpec {
            kind: ModelPackKind::OnnxTransformerMeanPool,
            model_id: MULTILINGUAL_MODEL_ID.into(),
            model_revision: MULTILINGUAL_REVISION.into(),
            tokenizer: MULTILINGUAL_TOKENIZER.into(),
            dimensions: MULTILINGUAL_DIMENSIONS,
            max_input_tokens: MULTILINGUAL_MAX_INPUT_TOKENS,
            query_prefix: MULTILINGUAL_QUERY_PREFIX.into(),
            passage_prefix: MULTILINGUAL_PASSAGE_PREFIX.into(),
            production: false,
            source: format!("huggingface:{MULTILINGUAL_REPOSITORY}@{MULTILINGUAL_REVISION}"),
            files: MULTILINGUAL_FILES
                .iter()
                .map(|file| (file.name.to_owned(), model_cache.path().join(file.name)))
                .collect(),
        },
    )?;
    if verify_pinned_pack {
        verify_packed_multilingual_model(&multilingual_pack)?;
    }
    let multilingual_bytes = fs::metadata(&multilingual_pack)?.len();
    let multilingual_checksum = digest_of(&multilingual_pack)?;
    let multilingual_id = content_addressed_id(
        "procyon.dev.model.multilingual-e5-small.v1",
        multilingual_checksum,
    )?;
    let multilingual_pack = staging.join("artifacts").join(multilingual_id.as_str());
    fs::rename(
        staging.join("artifacts").join(".multilingual-model-pack"),
        &multilingual_pack,
    )?;
    let multilingual_identity = ModelIdentity::new(
        ModelId::new(MULTILINGUAL_MODEL_ID)?,
        ModelRevision::new(MULTILINGUAL_REVISION)?,
    );
    let multilingual_metadata = ModelMetadata::new(
        multilingual_identity.clone(),
        LicenseInfo::new(
            "MIT",
            "intfloat/multilingual-e5-small, MIT licensed, redistributed unmodified \
             at the pinned upstream revision.",
        )?,
        TokenizerId::new(MULTILINGUAL_TOKENIZER)?,
        MULTILINGUAL_DIMENSIONS,
        EmbeddingNormalization::UnitLength,
        RuntimeCompatibility::new(
            runtime_component.clone(),
            VersionReq::parse(&format!("={version}"))?,
        ),
        MULTILINGUAL_LANGUAGES,
        multilingual_bytes,
        MULTILINGUAL_RAM_BYTES,
    )?;

    let worker_artifact = CatalogArtifact::new(
        worker_id.clone(),
        ComponentId::new("procyon.dev.worker")?,
        ArtifactKind::Worker,
        version.clone(),
        development_location(&worker_id)?,
        LicenseInfo::new("MIT", "Procyon semantic worker development build.")?,
        worker_checksum,
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
        runtime_checksum,
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
        model_checksum,
        resources(&model_bytes, model_metadata.estimated_ram_bytes())?,
        ArtifactCompatibility::new(
            None,
            None,
            vec![model_metadata.runtime().clone()],
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let multilingual_artifact = CatalogArtifact::new(
        multilingual_id.clone(),
        ComponentId::new("procyon.dev.model.multilingual-e5-small")?,
        ArtifactKind::Model(multilingual_identity.clone()),
        Version::new(1, 0, 0),
        development_location(&multilingual_id)?,
        multilingual_metadata.license().clone(),
        multilingual_checksum,
        ComponentResources::new(
            multilingual_bytes,
            multilingual_bytes,
            multilingual_metadata.estimated_ram_bytes(),
        )?,
        ArtifactCompatibility::new(
            None,
            None,
            vec![multilingual_metadata.runtime().clone()],
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let model_manifest = ModelManifest::new(model_id, model_metadata);
    let multilingual_manifest = ModelManifest::new(multilingual_id, multilingual_metadata);
    // The compact profiles keep the zero-download deterministic fixture so the
    // pipeline stays testable offline; only the explicit quality profile pulls
    // the real multi-hundred-megabyte multilingual model.
    let profiles = BTreeMap::from([
        (
            SemanticProfile::CompactMultilingual,
            model_manifest.metadata().identity().clone(),
        ),
        (
            SemanticProfile::CompactEnglish,
            model_manifest.metadata().identity().clone(),
        ),
        (
            SemanticProfile::MultilingualQuality,
            multilingual_manifest.metadata().identity().clone(),
        ),
    ]);
    let mut revision_material = Vec::with_capacity(4 * 32);
    for checksum in [
        worker_checksum,
        runtime_checksum,
        model_checksum,
        multilingual_checksum,
    ] {
        revision_material.extend_from_slice(checksum.as_bytes());
    }
    let revision_checksum = Sha256Digest::calculate(&revision_material);
    let manifest = CatalogManifest::new(
        ManifestRevision::new(format!(
            "procyon-dev-{target_label}-{}-{}",
            env!("CARGO_PKG_VERSION"),
            digest_prefix(revision_checksum)
        ))?,
        vec![
            worker_artifact,
            runtime_artifact,
            model_artifact,
            multilingual_artifact,
        ],
        vec![model_manifest, multilingual_manifest],
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

fn content_addressed_id(
    prefix: &str,
    checksum: Sha256Digest,
) -> Result<ArtifactId, Box<dyn std::error::Error>> {
    Ok(ArtifactId::new(format!(
        "{prefix}.sha256.{}",
        digest_prefix(checksum)
    ))?)
}

fn digest_prefix(checksum: Sha256Digest) -> String {
    let mut output = String::with_capacity(32);
    for byte in &checksum.as_bytes()[..16] {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
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

fn digest_of(path: &Path) -> io::Result<Sha256Digest> {
    use sha2::{Digest, Sha256};

    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_bytes(hasher.finalize().into()))
}

fn verify_multilingual_cache(
    directory: &Path,
) -> Result<VerifiedModelCache, Box<dyn std::error::Error>> {
    for descriptor in MULTILINGUAL_FILES {
        verify_pinned_file(&directory.join(descriptor.name), descriptor)?;
    }
    Ok(VerifiedModelCache(directory.to_owned()))
}

fn verify_packed_multilingual_model(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let pack = ModelPack::open(path)?;
    for descriptor in MULTILINGUAL_FILES {
        let member = pack.index().file(descriptor.name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("packed model is missing {}", descriptor.name),
            )
        })?;
        if member.length != descriptor.bytes
            || member.checksum.as_bytes() != &parse_sha256(descriptor.sha256)?
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "packed {} does not match revision {}",
                    descriptor.name, MULTILINGUAL_REVISION
                ),
            )
            .into());
        }
        pack.verify_member(descriptor.name)?;
    }
    Ok(())
}

fn verify_pinned_file(path: &Path, descriptor: PinnedModelFile) -> io::Result<()> {
    let metadata = fs::metadata(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "verified model cache is missing {}; run `pnpm semantic:model:fetch` first: {error}",
                descriptor.name
            ),
        )
    })?;
    if metadata.len() != descriptor.bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} has {} bytes but the pinned revision requires {}",
                descriptor.name,
                metadata.len(),
                descriptor.bytes
            ),
        ));
    }
    let expected = parse_sha256(descriptor.sha256)?;
    let actual = digest_of(path)?;
    if actual.as_bytes() != &expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} does not match the SHA-256 pinned for revision {}",
                descriptor.name, MULTILINGUAL_REVISION
            ),
        ));
    }
    Ok(())
}

fn parse_sha256(value: &str) -> io::Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pinned SHA-256 must contain 64 hexadecimal characters",
        ));
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes[index] = (high << 4) | low;
    }
    Ok(bytes)
}

fn hex_nibble(value: u8) -> io::Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pinned SHA-256 contains a non-hexadecimal character",
        )),
    }
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
    use fm_semantic_components::{ModelPack, ModelPackKind, SignedCatalogManifest, TrustedCatalog};

    use super::*;

    /// Writes stand-in files with the real member names so catalog assembly can
    /// be tested without the pinned multi-hundred-megabyte download.
    fn model_cache(directory: &Path) -> VerifiedModelCache {
        let cache = directory.join("model-cache");
        fs::create_dir_all(&cache).unwrap();
        for file in MULTILINGUAL_FILES {
            fs::write(
                cache.join(file.name),
                format!("fixture bytes for {}", file.name),
            )
            .unwrap();
        }
        VerifiedModelCache(cache)
    }

    fn trusted_catalog(output: &Path) -> TrustedCatalog {
        let manifest: CatalogManifest =
            serde_json::from_slice(&fs::read(output.join("catalog.json")).unwrap()).unwrap();
        let signature: [u8; 64] = fs::read(output.join("catalog.sig"))
            .unwrap()
            .try_into()
            .unwrap();
        let signing_key = SigningKey::from_bytes(&DEVELOPMENT_SIGNING_KEY);
        TrustedCatalog::verify(
            SignedCatalogManifest::new(manifest, signature),
            &signing_key.verifying_key(),
        )
        .unwrap()
    }

    fn build_fixture_bundle(
        worker: &Path,
        runtime: &Path,
        cache: &VerifiedModelCache,
        output: &Path,
    ) {
        build_bundle_inner(worker, runtime, cache, output, false).unwrap();
    }

    #[test]
    fn builds_a_complete_verifiable_development_catalog() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("fm-semantic-worker");
        let runtime = directory.path().join("libzvec_c_api.fixture");
        let output = directory.path().join("bundle");
        fs::write(&worker, b"development worker fixture").unwrap();
        fs::write(&runtime, b"development Zvec fixture").unwrap();
        build_fixture_bundle(&worker, &runtime, &model_cache(directory.path()), &output);

        let trusted = trusted_catalog(&output);

        assert_eq!(trusted.artifacts().len(), 4);
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
    fn pinned_cache_verification_rejects_a_member_changed_after_download() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("member");
        fs::write(&path, b"verified bytes").unwrap();
        let digest = digest_of(&path).unwrap();
        let descriptor = PinnedModelFile {
            name: "member",
            bytes: 14,
            sha256: "186287b2d987891f027b4bc8baaf621a3e5a4a73ec78e04b0f65dc309b1ccc03",
        };
        assert_eq!(digest.as_bytes(), &parse_sha256(descriptor.sha256).unwrap());
        verify_pinned_file(&path, descriptor).unwrap();

        fs::write(&path, b"tampered bytes").unwrap();
        assert!(verify_pinned_file(&path, descriptor).is_err());
    }

    #[test]
    fn maps_only_the_quality_profile_to_the_real_multilingual_model() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("fm-semantic-worker");
        let runtime = directory.path().join("libzvec_c_api.fixture");
        let output = directory.path().join("bundle");
        fs::write(&worker, b"development worker fixture").unwrap();
        fs::write(&runtime, b"development Zvec fixture").unwrap();
        build_fixture_bundle(&worker, &runtime, &model_cache(directory.path()), &output);

        let trusted = trusted_catalog(&output);
        let quality_identity = trusted
            .resolve_profile(SemanticProfile::MultilingualQuality)
            .unwrap()
            .clone();
        assert_eq!(quality_identity.model_id().as_str(), MULTILINGUAL_MODEL_ID);
        assert_eq!(quality_identity.revision().as_str(), MULTILINGUAL_REVISION);
        let quality = trusted.model(&quality_identity).unwrap().metadata();
        assert_eq!(quality.tokenizer().as_str(), MULTILINGUAL_TOKENIZER);
        assert_eq!(quality.dimensions(), MULTILINGUAL_DIMENSIONS);
        assert_eq!(quality.license().spdx(), "MIT");
        assert!(quality.language_coverage().contains(&"nl".to_owned()));
        for compact in [
            SemanticProfile::CompactMultilingual,
            SemanticProfile::CompactEnglish,
        ] {
            assert_eq!(
                trusted
                    .resolve_profile(compact)
                    .unwrap()
                    .model_id()
                    .as_str(),
                "procyon.dev.hashing-embedding"
            );
        }

        // One declared figure, quoted verbatim by docs/semantic-operations.md.
        assert_eq!(quality.estimated_ram_bytes(), MULTILINGUAL_RAM_BYTES);

        // The offered download is the real installed size, not a placeholder.
        let quality_artifact = trusted
            .artifacts()
            .iter()
            .find(|artifact| {
                matches!(artifact.kind(), ArtifactKind::Model(identity)
                    if identity.model_id().as_str() == MULTILINGUAL_MODEL_ID)
            })
            .unwrap();
        let pack_path = output
            .join("artifacts")
            .join(quality_artifact.id().as_str());
        assert_eq!(
            quality_artifact.resources().download_bytes(),
            fs::metadata(&pack_path).unwrap().len()
        );
        assert_eq!(
            quality_artifact.resources().ram_bytes(),
            MULTILINGUAL_RAM_BYTES
        );

        let pack = ModelPack::open(&pack_path).unwrap();
        assert_eq!(pack.index().kind, ModelPackKind::OnnxTransformerMeanPool);
        assert!(!pack.index().production);
        assert_eq!(pack.index().query_prefix, "query: ");
        assert_eq!(pack.index().passage_prefix, "passage: ");
        assert_eq!(pack.index().max_input_tokens, 512);
        for file in MULTILINGUAL_FILES {
            assert_eq!(
                pack.read(file.name).unwrap(),
                format!("fixture bytes for {}", file.name).into_bytes()
            );
        }
    }

    #[test]
    fn replaces_stale_bundle_contents_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let worker = directory.path().join("fm-semantic-worker");
        let runtime = directory.path().join("libzvec_c_api.fixture");
        let output = directory.path().join("bundle");
        let cache = model_cache(directory.path());
        fs::write(&worker, b"first worker").unwrap();
        fs::write(&runtime, b"development Zvec fixture").unwrap();
        build_fixture_bundle(&worker, &runtime, &cache, &output);
        let first_catalog = trusted_catalog(&output);
        let first_worker_id = first_catalog
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .unwrap()
            .id()
            .clone();
        let first_revision = first_catalog.revision().clone();
        fs::write(output.join("stale"), b"stale").unwrap();

        fs::write(&worker, b"second worker").unwrap();
        build_fixture_bundle(&worker, &runtime, &cache, &output);

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
        assert_ne!(worker_artifact.id(), &first_worker_id);
        assert_ne!(trusted.revision(), &first_revision);
        assert!(worker_artifact.id().as_str().contains(".sha256."));
    }
}
