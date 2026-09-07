//! Public acceptance tests for managed semantic components.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ed25519_dalek::{Signer, SigningKey};
use fm_semantic_components::SemanticProfile;
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactCompatibility, ArtifactId,
    ArtifactKind, ArtifactLocation, ArtifactRequest, ArtifactSource, ArtifactSourceError,
    CatalogArtifact, CatalogError, CatalogManifest, ComponentId, ComponentLifecycleStatus,
    ComponentManager, ComponentQuiescer, ComponentResources, DataCategory,
    DataMigrationCancellation, DeletionTarget, EmbeddingNormalization, EnrolmentDeletionCounts,
    EnrolmentDeletionPlan, EnrolmentId, FilesystemDurability, FreeSpaceError, FreeSpaceProbe,
    IndexingController, IndexingPauseGuard, InstallEnvironment, InstallError, LicenseInfo,
    LocalModelImport, LocalModelImportRequest, ManifestRevision, ModelId, ModelIdentity,
    ModelImportError, ModelImportField, ModelManifest, ModelMetadata, ModelRevision, PauseError,
    ProductionArtifactProvenance, ProductionCatalogError, ProductionCatalogManifest,
    ProductionPipelineIdentity, ProtocolRange, QuiesceError, ReindexEstimate, ReindexReason,
    RuntimeCompatibility, SemanticDataRoot, SemanticStateError, SemanticStateStore, Sha256Digest,
    SignedCatalogManifest, SignedProductionCatalogManifest, TargetTriple, TokenizerId,
    TrustedCatalog, UninstallIndexDecision, production_artifact_id, sign_production_catalog,
    verify_production_payloads, verify_serialized_production_catalog,
    write_signed_production_catalog,
};
use semver::{Version, VersionReq};
use tempfile::TempDir;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/component-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    let parent = parent
        .canonicalize()
        .expect("canonicalize project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

#[test]
fn default_state_downloads_nothing_and_installs_nothing() {
    let directory = project_temp_dir("default-");
    let app_data = directory.path().join("app-data");
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let state = manager.state().expect("default state");

    assert!(state.installed_components().is_empty());
    assert!(state.active_model().is_none());
    assert!(!state.data_root().path().exists());
}

#[test]
fn lifecycle_lock_child_process() {
    let Some(config) = std::env::var_os("PROCYON_SEMANTIC_LOCK_TEST_CONFIG") else {
        return;
    };
    let app_data = std::env::var_os("PROCYON_SEMANTIC_LOCK_TEST_APP_DATA").unwrap();
    let marker = std::env::var_os("PROCYON_SEMANTIC_LOCK_TEST_MARKER").unwrap();
    let manager = ComponentManager::new(SemanticStateStore::new(config), app_data);

    manager
        .run_serialized_lifecycle(|| {
            std::fs::write(marker, b"locked")?;
            thread::sleep(Duration::from_millis(400));
            Ok::<(), SemanticStateError>(())
        })
        .unwrap();
}

#[test]
fn lifecycle_mutations_are_serialized_across_processes() {
    let directory = project_temp_dir("cross-process-lock-");
    let config = directory.path().join("config");
    let app_data = directory.path().join("app-data");
    let marker = directory.path().join("child-holds-lock");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("lifecycle_lock_child_process")
        .arg("--nocapture")
        .env("PROCYON_SEMANTIC_LOCK_TEST_CONFIG", &config)
        .env("PROCYON_SEMANTIC_LOCK_TEST_APP_DATA", &app_data)
        .env("PROCYON_SEMANTIC_LOCK_TEST_MARKER", &marker)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if marker.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "child never acquired the lifecycle lock");

    let manager = ComponentManager::new(SemanticStateStore::new(config), app_data);
    let started = std::time::Instant::now();
    manager
        .run_serialized_lifecycle(|| Ok::<(), SemanticStateError>(()))
        .unwrap();
    let elapsed = started.elapsed();
    let status = child.wait().unwrap();

    assert!(status.success());
    assert!(
        elapsed >= Duration::from_millis(250),
        "cross-process lifecycle lock released after only {elapsed:?}"
    );
}

#[test]
fn setup_profiles_are_abstract_and_compact_multilingual_is_recommended() {
    assert_eq!(
        SemanticProfile::recommended(),
        SemanticProfile::CompactMultilingual
    );
    assert_eq!(
        SemanticProfile::all(),
        &[
            SemanticProfile::CompactMultilingual,
            SemanticProfile::CompactEnglish,
            SemanticProfile::MultilingualQuality,
        ]
    );
    assert!(
        SemanticProfile::all()
            .iter()
            .all(|profile| !profile.explanation().is_empty())
    );
}

#[test]
fn path_component_identifiers_reject_traversal_separators_and_absolute_paths() {
    for invalid in [
        ".",
        "..",
        "../outside",
        "nested/name",
        r"nested\name",
        "/outside",
        "bad\0id",
        "NUL",
        "unsafe.",
    ] {
        assert!(
            ArtifactId::new(invalid).is_err(),
            "accepted artifact id {invalid:?}"
        );
        assert!(
            ComponentId::new(invalid).is_err(),
            "accepted component id {invalid:?}"
        );
        assert!(
            ModelId::new(invalid).is_err(),
            "accepted model id {invalid:?}"
        );
        assert!(
            ModelRevision::new(invalid).is_err(),
            "accepted model revision {invalid:?}"
        );
        assert!(
            ManifestRevision::new(invalid).is_err(),
            "accepted manifest revision {invalid:?}"
        );
        assert!(
            TokenizerId::new(invalid).is_err(),
            "accepted tokenizer id {invalid:?}"
        );
        assert!(
            EnrolmentId::new(invalid).is_err(),
            "accepted enrolment id {invalid:?}"
        );
    }
}

#[test]
fn curated_catalog_requires_a_valid_ed25519_signature_over_canonical_bytes() {
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let manifest = CatalogManifest::new(
        ManifestRevision::new("fixture-catalog-1").expect("valid revision"),
        Vec::new(),
        Vec::new(),
        BTreeMap::new(),
    )
    .expect("valid fixture manifest");
    let signature = signing_key.sign(
        &manifest
            .canonical_bytes()
            .expect("manifest has canonical bytes"),
    );

    let signed = SignedCatalogManifest::new(manifest.clone(), signature.to_bytes());
    let trusted = TrustedCatalog::verify(signed, &signing_key.verifying_key())
        .expect("valid signature is accepted");
    assert_eq!(trusted.revision(), manifest.revision());

    let tampered = SignedCatalogManifest::new(
        CatalogManifest::new(
            ManifestRevision::new("fixture-catalog-2").expect("valid revision"),
            Vec::new(),
            Vec::new(),
            BTreeMap::new(),
        )
        .expect("valid tampered manifest"),
        signature.to_bytes(),
    );
    assert!(matches!(
        TrustedCatalog::verify(tampered, &signing_key.verifying_key()),
        Err(CatalogError::InvalidSignature)
    ));
}

#[test]
fn deserialized_catalog_locations_cannot_bypass_credential_free_https_validation() {
    for invalid in [
        "\"http://fixtures.invalid/model\"",
        "\"https://user:secret@fixtures.invalid/model\"",
        "\"https://fixtures.invalid/model?token=secret\"",
        "\"https://fixtures.invalid/model#fragment\"",
    ] {
        assert!(
            serde_json::from_str::<ArtifactLocation>(invalid).is_err(),
            "accepted unsafe catalog location {invalid}"
        );
    }
}

#[test]
fn normal_catalog_model_metadata_records_the_exact_embedding_space_contract() {
    let identity = ModelIdentity::new(
        ModelId::new("fixture.embedding.multilingual").expect("valid model id"),
        ModelRevision::new("upstream-commit-deadbeef").expect("valid immutable revision"),
    );
    let metadata = ModelMetadata::new(
        identity.clone(),
        LicenseInfo::new("Apache-2.0", "Fixture model notice").expect("valid license"),
        TokenizerId::new("fixture-tokenizer-v1").expect("valid tokenizer"),
        384,
        EmbeddingNormalization::UnitLength,
        RuntimeCompatibility::new(
            ComponentId::new("fixture.runtime").expect("valid component id"),
            VersionReq::parse(">=1.4.0, <2.0.0").expect("valid version requirement"),
        ),
        ["en", "nl", "ja"],
        280_000_000,
        420_000_000,
    )
    .expect("complete metadata is valid");

    assert_eq!(metadata.identity(), &identity);
    assert_eq!(metadata.license().spdx(), "Apache-2.0");
    assert_eq!(metadata.tokenizer().as_str(), "fixture-tokenizer-v1");
    assert_eq!(metadata.dimensions(), 384);
    assert_eq!(metadata.normalization(), EmbeddingNormalization::UnitLength);
    assert_eq!(
        metadata.runtime().component_id().as_str(),
        "fixture.runtime"
    );
    assert_eq!(
        metadata.language_coverage(),
        &["en".to_owned(), "ja".to_owned(), "nl".to_owned()]
    );
    assert_eq!(metadata.estimated_disk_bytes(), 280_000_000);
    assert_eq!(metadata.estimated_ram_bytes(), 420_000_000);
}

fn fixture_model_metadata() -> ModelMetadata {
    ModelMetadata::new(
        ModelIdentity::new(
            ModelId::new("fixture.embedding.multilingual").expect("valid model id"),
            ModelRevision::new("upstream-commit-deadbeef").expect("valid revision"),
        ),
        LicenseInfo::new("Apache-2.0", "Fixture model notice").expect("valid license"),
        TokenizerId::new("fixture-tokenizer-v1").expect("valid tokenizer"),
        384,
        EmbeddingNormalization::UnitLength,
        RuntimeCompatibility::new(
            ComponentId::new("fixture.runtime").expect("valid component id"),
            VersionReq::parse(">=1.4.0, <2.0.0").expect("valid requirement"),
        ),
        ["en", "nl", "ja"],
        280_000_000,
        420_000_000,
    )
    .expect("complete metadata")
}

fn fixture_catalog_manifest() -> CatalogManifest {
    let target = TargetTriple::new("macos", "aarch64").expect("valid target");
    let worker = CatalogArtifact::new(
        ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").expect("valid artifact id"),
        ComponentId::new("fixture.worker").expect("valid component id"),
        ArtifactKind::Worker,
        Version::parse("1.2.0").expect("valid version"),
        ArtifactLocation::new("https://fixtures.invalid/worker").expect("valid signed location"),
        LicenseInfo::new("MIT", "Fixture worker notice").expect("valid license"),
        Sha256Digest::calculate(b"fixture worker"),
        ComponentResources::new(14, 26_000_000, 48_000_000).expect("valid resources"),
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 2).expect("valid protocol range")),
            Vec::new(),
            7,
        ),
    )
    .expect("valid worker");
    let runtime = CatalogArtifact::new(
        ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").expect("valid artifact id"),
        ComponentId::new("fixture.runtime").expect("valid component id"),
        ArtifactKind::Runtime,
        Version::parse("1.4.0").expect("valid version"),
        ArtifactLocation::new("https://fixtures.invalid/runtime").expect("valid signed location"),
        LicenseInfo::new("MIT", "Fixture runtime notice").expect("valid license"),
        Sha256Digest::calculate(b"fixture runtime"),
        ComponentResources::new(15, 44_000_000, 80_000_000).expect("valid resources"),
        ArtifactCompatibility::new(Some(target), None, Vec::new(), 7),
    )
    .expect("valid runtime");
    let metadata = fixture_model_metadata();
    let model_artifact_id =
        ArtifactId::new("fixture.model.multilingual.deadbeef").expect("valid artifact id");
    let model = CatalogArtifact::new(
        model_artifact_id.clone(),
        ComponentId::new("fixture.model.multilingual").expect("valid component id"),
        ArtifactKind::Model(metadata.identity().clone()),
        Version::parse("1.0.0").expect("valid package version"),
        ArtifactLocation::new("https://fixtures.invalid/model").expect("valid signed location"),
        metadata.license().clone(),
        Sha256Digest::calculate(b"fixture model"),
        ComponentResources::new(
            13,
            metadata.estimated_disk_bytes(),
            metadata.estimated_ram_bytes(),
        )
        .expect("valid resources"),
        ArtifactCompatibility::new(None, None, vec![metadata.runtime().clone()], 7),
    )
    .expect("valid model artifact");
    let model_manifest = ModelManifest::new(model_artifact_id, metadata);
    let profiles = [(
        SemanticProfile::CompactMultilingual,
        model_manifest.metadata().identity().clone(),
    )]
    .into();
    CatalogManifest::new(
        ManifestRevision::new("fixture-catalog-complete").expect("valid revision"),
        vec![worker, runtime, model],
        vec![model_manifest],
        profiles,
    )
    .expect("valid complete manifest")
}

fn fixture_production_manifest(
    reverse_provenance: bool,
) -> Result<ProductionCatalogManifest, CatalogError> {
    let model = fixture_model_metadata();
    let pipeline = ProductionPipelineIdentity::new(
        1,
        7,
        "docling-pdf/1036000+baseline/1",
        "structural/2",
        model.tokenizer().clone(),
        model.identity().clone(),
    )?;
    let mut provenance = vec![
        ProductionArtifactProvenance::new(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-0")?,
            ArtifactLocation::new("https://github.com/example/procyon")?,
            ManifestRevision::new("commit-worker-deadbeef")?,
        ),
        ProductionArtifactProvenance::new(
            ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0")?,
            ArtifactLocation::new("https://github.com/example/zvec")?,
            ManifestRevision::new("tag-zvec-1-4-0")?,
        ),
        ProductionArtifactProvenance::new(
            ArtifactId::new("fixture.model.multilingual.deadbeef")?,
            ArtifactLocation::new("https://huggingface.co/example/model")?,
            ManifestRevision::new("commit-model-deadbeef")?,
        ),
    ];
    if reverse_provenance {
        provenance.reverse();
    }
    ProductionCatalogManifest::new(fixture_catalog_manifest(), pipeline, provenance)
}

#[test]
fn production_catalog_generation_is_deterministic_and_records_pipeline_provenance() {
    let first = fixture_production_manifest(false).expect("production manifest");
    let reordered = fixture_production_manifest(true).expect("production manifest");

    assert_eq!(
        first.canonical_bytes().expect("canonical catalog"),
        reordered.canonical_bytes().expect("canonical catalog")
    );
    assert_eq!(
        first.pipeline().converter(),
        "docling-pdf/1036000+baseline/1"
    );
    assert_eq!(first.pipeline().chunker(), "structural/2");
    assert_eq!(first.provenance().len(), 3);
}

#[test]
fn production_catalog_signature_covers_provenance_and_pipeline_compatibility() {
    let signing_key = SigningKey::from_bytes(&[23_u8; 32]);
    let manifest = fixture_production_manifest(false).expect("production manifest");
    let signature = signing_key.sign(&manifest.canonical_bytes().expect("canonical catalog"));
    let signed = SignedProductionCatalogManifest::new(manifest.clone(), signature.to_bytes());

    let trusted = TrustedCatalog::verify_production(signed, &signing_key.verifying_key())
        .expect("valid production catalog");
    assert_eq!(trusted.revision(), manifest.catalog().revision());

    let mut altered = serde_json::to_value(manifest).expect("serialize production manifest");
    altered["pipeline"]["chunker"] = "structural/3".into();
    let altered = serde_json::from_value(altered).expect("deserialize altered manifest");
    let signed = SignedProductionCatalogManifest::new(altered, signature.to_bytes());
    assert!(matches!(
        TrustedCatalog::verify_production(signed, &signing_key.verifying_key()),
        Err(CatalogError::InvalidSignature)
    ));
}

#[test]
fn production_catalog_rejects_incomplete_provenance_and_identity_drift() {
    let manifest = fixture_production_manifest(false).expect("production manifest");
    let value = serde_json::to_value(&manifest).expect("serialize production manifest");

    let mut missing_source = value.clone();
    missing_source["provenance"].as_array_mut().unwrap().pop();
    let missing_source: ProductionCatalogManifest =
        serde_json::from_value(missing_source).expect("structurally valid manifest");
    assert!(matches!(
        missing_source.validate(),
        Err(CatalogError::MissingProductionProvenance { .. })
    ));

    let mut wrong_tokenizer = value.clone();
    wrong_tokenizer["pipeline"]["tokenizer"] = "different-tokenizer".into();
    let wrong_tokenizer: ProductionCatalogManifest =
        serde_json::from_value(wrong_tokenizer).expect("structurally valid manifest");
    assert!(matches!(
        wrong_tokenizer.validate(),
        Err(CatalogError::ProductionCompatibilityMismatch { field: "tokenizer" })
    ));

    let mut wrong_schema = value;
    wrong_schema["pipeline"]["index_schema_version"] = 8.into();
    let wrong_schema: ProductionCatalogManifest =
        serde_json::from_value(wrong_schema).expect("structurally valid manifest");
    assert!(matches!(
        wrong_schema.validate(),
        Err(CatalogError::ProductionCompatibilityMismatch {
            field: "index schema"
        })
    ));
}

#[test]
fn production_catalog_rejects_a_signed_converter_or_chunker_for_another_host_pipeline() {
    let signing_key = SigningKey::from_bytes(&[29_u8; 32]);
    let manifest = fixture_production_manifest(false).expect("production manifest");
    let expected_model = fixture_model_metadata();
    for (converter, chunker, expected_field) in [
        (
            "docling-pdf/1036001+baseline/1",
            "structural/2",
            "converter",
        ),
        ("docling-pdf/1036000+baseline/1", "structural/3", "chunker"),
    ] {
        let incompatible = ProductionPipelineIdentity::new(
            1,
            7,
            converter,
            chunker,
            expected_model.tokenizer().clone(),
            expected_model.identity().clone(),
        )
        .unwrap();
        let signature = signing_key.sign(&manifest.canonical_bytes().unwrap());
        let signed = SignedProductionCatalogManifest::new(manifest.clone(), signature.to_bytes());
        assert!(matches!(
            TrustedCatalog::verify_production_for_pipeline(
                signed,
                &signing_key.verifying_key(),
                &incompatible,
            ),
            Err(CatalogError::ProductionCompatibilityMismatch { field })
                if field == expected_field
        ));
    }
}

#[test]
fn production_catalog_signing_verifies_exact_payloads_and_writes_repeatable_outputs() {
    let directory = project_temp_dir("production-catalog-");
    let artifacts = directory.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    for (id, bytes) in [
        (
            "fixture.worker.macos-aarch64.1-2-0",
            b"fixture worker".as_slice(),
        ),
        (
            "fixture.runtime.macos-aarch64.1-4-0",
            b"fixture runtime".as_slice(),
        ),
        (
            "fixture.model.multilingual.deadbeef",
            b"fixture model".as_slice(),
        ),
    ] {
        std::fs::write(artifacts.join(id), bytes).unwrap();
    }
    let manifest = fixture_production_manifest(false).expect("production manifest");
    verify_production_payloads(&manifest, &artifacts).expect("exact payload set");

    let signing_key = SigningKey::from_bytes(&[31_u8; 32]);
    let signed = sign_production_catalog(manifest, &signing_key).expect("sign catalog");
    let first = directory.path().join("first");
    let second = directory.path().join("second");
    write_signed_production_catalog(&signed, &first).expect("first output");
    write_signed_production_catalog(&signed, &second).expect("second output");
    assert_eq!(
        std::fs::read(first.join("catalog.json")).unwrap(),
        std::fs::read(second.join("catalog.json")).unwrap()
    );
    assert_eq!(
        std::fs::read(first.join("catalog.sig")).unwrap(),
        std::fs::read(second.join("catalog.sig")).unwrap()
    );
    verify_serialized_production_catalog(
        &std::fs::read(first.join("catalog.json")).unwrap(),
        &std::fs::read(first.join("catalog.sig")).unwrap(),
        &signing_key.verifying_key(),
    )
    .expect("public-key verification");

    std::fs::write(
        artifacts.join("fixture.model.multilingual.deadbeef"),
        b"tampered mode",
    )
    .unwrap();
    assert!(matches!(
        verify_production_payloads(signed.manifest(), &artifacts),
        Err(ProductionCatalogError::PayloadChecksumMismatch { .. })
    ));
}

#[test]
fn production_payload_verification_rejects_truncated_and_unknown_files() {
    let directory = project_temp_dir("production-payload-rejection-");
    let artifacts = directory.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).unwrap();
    for (id, bytes) in [
        (
            "fixture.worker.macos-aarch64.1-2-0",
            b"fixture worker".as_slice(),
        ),
        (
            "fixture.runtime.macos-aarch64.1-4-0",
            b"fixture runtime".as_slice(),
        ),
        (
            "fixture.model.multilingual.deadbeef",
            b"fixture mode".as_slice(),
        ),
    ] {
        std::fs::write(artifacts.join(id), bytes).unwrap();
    }
    let manifest = fixture_production_manifest(false).expect("production manifest");
    assert!(matches!(
        verify_production_payloads(&manifest, &artifacts),
        Err(ProductionCatalogError::PayloadSizeMismatch { .. })
    ));

    std::fs::write(
        artifacts.join("fixture.model.multilingual.deadbeef"),
        b"fixture model",
    )
    .unwrap();
    std::fs::write(artifacts.join("not-in-catalog"), b"unknown").unwrap();
    assert!(matches!(
        verify_production_payloads(&manifest, &artifacts),
        Err(ProductionCatalogError::UnknownPayload { .. })
    ));
}

#[test]
fn production_artifact_ids_bind_component_target_version_and_payload() {
    let component = ComponentId::new("procyon.semantic.worker").unwrap();
    let target = TargetTriple::new("linux", "x86_64").unwrap();
    let version = Version::parse("0.1.0-23").unwrap();
    let first = production_artifact_id(
        &component,
        Some(&target),
        &version,
        Sha256Digest::calculate(b"worker-a"),
    )
    .unwrap();
    let second = production_artifact_id(
        &component,
        Some(&target),
        &version,
        Sha256Digest::calculate(b"worker-b"),
    )
    .unwrap();

    assert!(
        first
            .as_str()
            .starts_with("procyon.semantic.worker.linux-x86_64.0.1.0.23.")
    );
    assert_ne!(first, second);
}

fn signed_fixture_catalog() -> TrustedCatalog {
    trust_manifest(fixture_catalog_manifest())
}

fn signed_fixture_catalog_with_schema(index_schema_version: u32) -> TrustedCatalog {
    let mut manifest = serde_json::to_value(fixture_catalog_manifest()).unwrap();
    for artifact in manifest["artifacts"].as_array_mut().unwrap() {
        artifact["compatibility"]["index_schema_version"] = index_schema_version.into();
    }
    trust_manifest(serde_json::from_value(manifest).unwrap())
}

fn trust_manifest(manifest: CatalogManifest) -> TrustedCatalog {
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let signature = signing_key.sign(&manifest.canonical_bytes().expect("canonical manifest"));
    TrustedCatalog::verify(
        SignedCatalogManifest::new(manifest, signature.to_bytes()),
        &signing_key.verifying_key(),
    )
    .expect("trusted fixture catalog")
}

fn verify_signed_manifest_value(value: serde_json::Value) -> Result<TrustedCatalog, CatalogError> {
    let manifest: CatalogManifest =
        serde_json::from_value(value).expect("mutation remains structurally deserializable");
    let signing_key = SigningKey::from_bytes(&[13_u8; 32]);
    let signature = signing_key.sign(&manifest.canonical_bytes().expect("canonical manifest"));
    TrustedCatalog::verify(
        SignedCatalogManifest::new(manifest, signature.to_bytes()),
        &signing_key.verifying_key(),
    )
}

type ManifestMutation = (&'static str, fn(&mut serde_json::Value));

#[test]
fn signed_catalog_revalidates_every_deserialized_nested_invariant() {
    let original = serde_json::to_value(fixture_catalog_manifest()).unwrap();
    let mutations: &[ManifestMutation] = &[
        ("unsupported format", |value: &mut serde_json::Value| {
            value["format_version"] = 2.into()
        }),
        ("unsafe target", |value| {
            value["artifacts"][0]["compatibility"]["target"]["operating_system"] = ".".into();
        }),
        ("reversed protocol", |value| {
            value["artifacts"][0]["compatibility"]["protocol"]["minimum"] = 3.into();
            value["artifacts"][0]["compatibility"]["protocol"]["maximum"] = 2.into();
        }),
        ("zero resources", |value| {
            value["artifacts"][0]["resources"]["download_bytes"] = 0.into();
        }),
        ("zero schema", |value| {
            value["artifacts"][0]["compatibility"]["index_schema_version"] = 0.into();
        }),
        ("case-folded artifact collision", |value| {
            let mut artifact = value["artifacts"][0].clone();
            artifact["id"] = value["artifacts"][0]["id"]
                .as_str()
                .unwrap()
                .to_ascii_uppercase()
                .into();
            value["artifacts"].as_array_mut().unwrap().push(artifact);
        }),
        ("blank license", |value| {
            value["artifacts"][0]["license"]["spdx"] = "".into();
        }),
        ("zero model dimensions", |value| {
            value["models"][0]["metadata"]["dimensions"] = 0.into();
        }),
        ("blank language", |value| {
            value["models"][0]["metadata"]["language_coverage"][0] = "".into();
        }),
        ("duplicate runtime dependency", |value| {
            let runtime = value["artifacts"][2]["compatibility"]["runtimes"][0].clone();
            value["artifacts"][2]["compatibility"]["runtimes"]
                .as_array_mut()
                .unwrap()
                .push(runtime);
        }),
        ("unknown profile model", |value| {
            value["profile_resolutions"]["compact-multilingual"]["model"] = "unknown-model".into();
        }),
    ];

    for (name, mutate) in mutations {
        let mut malformed = original.clone();
        mutate(&mut malformed);
        assert!(
            verify_signed_manifest_value(malformed).is_err(),
            "accepted signed manifest with {name}"
        );
    }
}

fn worker_artifact(
    id: &str,
    component: &str,
    version: &str,
    os: &str,
    schema: u32,
) -> CatalogArtifact {
    CatalogArtifact::new(
        ArtifactId::new(id).expect("valid artifact id"),
        ComponentId::new(component).expect("valid component id"),
        ArtifactKind::Worker,
        Version::parse(version).expect("valid version"),
        ArtifactLocation::new(format!("https://fixtures.invalid/{id}"))
            .expect("valid signed location"),
        LicenseInfo::new("MIT", "Fixture worker notice").expect("valid license"),
        Sha256Digest::calculate(id.as_bytes()),
        ComponentResources::new(10, 20, 30).expect("valid resources"),
        ArtifactCompatibility::new(
            Some(TargetTriple::new(os, "aarch64").expect("valid target")),
            Some(ProtocolRange::new(1, 2).expect("valid protocol range")),
            Vec::new(),
            schema,
        ),
    )
    .expect("valid worker")
}

#[test]
fn first_install_offer_discloses_signed_components_resources_privacy_and_location() {
    let catalog = signed_fixture_catalog();
    let offered = catalog
        .installation_offer(
            SemanticProfile::CompactMultilingual,
            &[
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
            ],
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            Path::new("/fixture/app-data/semantic"),
            750_000_000,
        )
        .expect("compatible signed offer");

    assert_eq!(offered.profile(), SemanticProfile::CompactMultilingual);
    assert_eq!(
        offered.resolved_model().revision().as_str(),
        "upstream-commit-deadbeef"
    );
    assert_eq!(offered.components().len(), 3);
    assert!(offered.components().iter().all(|component| {
        !component.version().to_string().is_empty()
            && !component.license().spdx().is_empty()
            && component.download_bytes() > 0
            && component.estimated_installed_bytes() > 0
            && component.estimated_ram_bytes() > 0
    }));
    assert!(offered.local_only_disclosure().embeddings_stay_local());
    assert!(!offered.local_only_disclosure().text().is_empty());
    assert_eq!(
        offered.semantic_data_root(),
        Path::new("/fixture/app-data/semantic")
    );
    assert_eq!(offered.minimum_free_space_reserve_bytes(), 750_000_000);
}

#[test]
fn install_offer_rejects_platform_protocol_and_runtime_incompatibility() {
    let catalog = signed_fixture_catalog();
    let ids = [
        ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
        ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
        ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
    ];
    let wrong_platform = catalog.installation_offer(
        SemanticProfile::CompactMultilingual,
        &ids,
        &TargetTriple::new("linux", "aarch64").unwrap(),
        1,
        Path::new("/fixture/semantic"),
        100,
    );
    assert!(matches!(
        wrong_platform,
        Err(CatalogError::IncompatibleTarget { .. })
    ));

    let wrong_protocol = catalog.installation_offer(
        SemanticProfile::CompactMultilingual,
        &ids,
        &TargetTriple::new("macos", "aarch64").unwrap(),
        9,
        Path::new("/fixture/semantic"),
        100,
    );
    assert!(matches!(
        wrong_protocol,
        Err(CatalogError::IncompatibleProtocol { .. })
    ));

    let missing_runtime = catalog.installation_offer(
        SemanticProfile::CompactMultilingual,
        &[ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap()],
        &TargetTriple::new("macos", "aarch64").unwrap(),
        1,
        Path::new("/fixture/semantic"),
        100,
    );
    assert!(matches!(
        missing_runtime,
        Err(CatalogError::IncompatibleRuntime { .. })
    ));
}

#[test]
fn patch_update_selection_handles_unsorted_interleaved_components_and_versions() {
    let manifest = CatalogManifest::new(
        ManifestRevision::new("fixture-updates").unwrap(),
        vec![
            worker_artifact("worker-linux-1-2-9", "fixture.worker", "1.2.9", "linux", 7),
            worker_artifact("other-worker-9-9-9", "other.worker", "9.9.9", "macos", 7),
            worker_artifact("worker-macos-1-3-0", "fixture.worker", "1.3.0", "macos", 7),
            worker_artifact("worker-macos-1-2-4", "fixture.worker", "1.2.4", "macos", 7),
            worker_artifact(
                "worker-macos-1-2-3-schema",
                "fixture.worker",
                "1.2.3",
                "macos",
                8,
            ),
            worker_artifact("worker-macos-1-1-9", "fixture.worker", "1.1.9", "macos", 7),
            worker_artifact("worker-macos-1-2-2", "fixture.worker", "1.2.2", "macos", 7),
        ],
        Vec::new(),
        BTreeMap::new(),
    )
    .expect("valid update catalog");
    let catalog = trust_manifest(manifest);

    let selected = catalog
        .select_worker_patch_update(
            &ComponentId::new("fixture.worker").unwrap(),
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
        )
        .expect("a compatible patch is available");

    assert_eq!(selected.version(), &Version::parse("1.2.4").unwrap());
}

#[test]
fn semantic_data_root_defaults_from_injected_app_data_and_separates_every_category() {
    let directory = project_temp_dir("layout-");
    let root = SemanticDataRoot::from_app_data(directory.path());

    assert_eq!(root.path(), directory.path().join("semantic"));
    root.initialize().expect("initialize semantic data layout");

    let names: Vec<&str> = DataCategory::all()
        .iter()
        .map(|category| category.directory_name())
        .collect();
    assert_eq!(
        names,
        [
            "catalog",
            "extracted",
            "zvec",
            "embedding-cache",
            "models",
            "workers",
        ]
    );
    assert!(
        DataCategory::all()
            .iter()
            .all(|category| root.category_path(*category).is_dir())
    );
}

#[test]
fn state_persists_abstract_profile_and_exact_resolved_model_revision_atomically() {
    let directory = project_temp_dir("state-");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let app_data = directory.path().join("app-data");
    let mut state = store
        .load_or_default(&app_data)
        .expect("load default semantic state");
    let identity = fixture_model_metadata().identity().clone();
    state
        .activate_initial_model(SemanticProfile::CompactMultilingual, identity.clone(), 7)
        .expect("activate first embedding space");

    store.save(&state).expect("save semantic state");
    let loaded = store
        .load_or_default(Path::new("/ignored/after-state-exists"))
        .expect("load persisted semantic state");

    assert_eq!(
        loaded.data_root().path(),
        app_data.join("semantic").as_path()
    );
    assert_eq!(
        loaded.active_model().expect("active model").profile(),
        SemanticProfile::CompactMultilingual
    );
    assert_eq!(
        loaded.active_model().expect("active model").identity(),
        &identity
    );
    assert!(
        std::fs::read_dir(directory.path().join("config"))
            .expect("read state directory")
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp"))
    );
}

#[test]
fn durable_semantic_state_has_no_credential_or_secret_fields() {
    let directory = project_temp_dir("state-fields-");
    let state = SemanticStateStore::new(directory.path().join("config"))
        .load_or_default(&directory.path().join("app-data"))
        .expect("default state");

    let serialized = serde_json::to_value(&state).expect("serialize typed state");
    fn keys(value: &serde_json::Value, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    output.push(key.to_ascii_lowercase());
                    keys(value, output);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    keys(value, output);
                }
            }
            _ => {}
        }
    }
    let mut field_names = Vec::new();
    keys(&serialized, &mut field_names);

    assert!(field_names.iter().all(|field| {
        !field.contains("credential")
            && !field.contains("password")
            && !field.contains("secret")
            && !field.contains("token")
    }));
}

fn valid_local_model_request(path: &Path) -> LocalModelImportRequest {
    LocalModelImportRequest {
        source_path: path.to_owned(),
        model_id: "expert.local.fixture".to_owned(),
        upstream_revision: "sha-deadbeef".to_owned(),
        license_spdx: "Apache-2.0".to_owned(),
        license_notice: "Local fixture notice".to_owned(),
        tokenizer: "fixture-tokenizer".to_owned(),
        dimensions: 768,
        normalization: Some(EmbeddingNormalization::UnitLength),
        runtime_component_id: "fixture.runtime".to_owned(),
        runtime_version_requirement: ">=1.4.0, <2.0.0".to_owned(),
        language_coverage: vec!["en".to_owned(), "nl".to_owned()],
        estimated_disk_bytes: 600_000_000,
        estimated_ram_bytes: 900_000_000,
    }
}

#[test]
fn expert_local_model_import_validates_every_required_metadata_field() {
    let directory = project_temp_dir("local-model-");
    let model_path = directory.path().join("model.bin");
    std::fs::write(&model_path, b"local fixture model").expect("write local model");
    let valid = valid_local_model_request(&model_path);
    LocalModelImport::validate(valid.clone()).expect("complete local metadata is accepted");

    let mut invalid = Vec::new();
    let mut request = valid.clone();
    request.model_id.clear();
    invalid.push((ModelImportField::ModelId, request));
    let mut request = valid.clone();
    request.upstream_revision.clear();
    invalid.push((ModelImportField::UpstreamRevision, request));
    let mut request = valid.clone();
    request.license_spdx.clear();
    invalid.push((ModelImportField::License, request));
    let mut request = valid.clone();
    request.tokenizer.clear();
    invalid.push((ModelImportField::Tokenizer, request));
    let mut request = valid.clone();
    request.dimensions = 0;
    invalid.push((ModelImportField::Dimensions, request));
    let mut request = valid.clone();
    request.normalization = None;
    invalid.push((ModelImportField::Normalization, request));
    let mut request = valid.clone();
    request.runtime_component_id.clear();
    invalid.push((ModelImportField::RuntimeCompatibility, request));
    let mut request = valid.clone();
    request.runtime_version_requirement.clear();
    invalid.push((ModelImportField::RuntimeCompatibility, request));
    let mut request = valid.clone();
    request.language_coverage.clear();
    invalid.push((ModelImportField::LanguageCoverage, request));
    let mut request = valid.clone();
    request.estimated_disk_bytes = 0;
    invalid.push((ModelImportField::EstimatedDiskBytes, request));
    let mut request = valid;
    request.estimated_ram_bytes = 0;
    invalid.push((ModelImportField::EstimatedRamBytes, request));

    for (field, request) in invalid {
        assert!(matches!(
            LocalModelImport::validate(request),
            Err(ModelImportError::MissingMetadata { field: actual }) if actual == field
        ));
    }
}

#[test]
fn local_model_import_requires_an_explicit_resumable_full_reindex_before_activation() {
    let directory = project_temp_dir("model-migration-");
    let model_path = directory.path().join("model.bin");
    std::fs::write(&model_path, b"local fixture model").expect("write local model");
    let import =
        LocalModelImport::validate(valid_local_model_request(&model_path)).expect("valid import");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let mut state = store
        .load_or_default(&directory.path().join("app-data"))
        .expect("default state");
    let original = ModelIdentity::new(
        ModelId::new("fixture.original").unwrap(),
        ModelRevision::new("revision-one").unwrap(),
    );
    state
        .activate_initial_model(SemanticProfile::CompactMultilingual, original.clone(), 7)
        .expect("initial activation");

    let plan = state
        .plan_local_model_migration(
            &import,
            SemanticProfile::CompactEnglish,
            ReindexEstimate::new(120, 12_000_000),
        )
        .expect("distinct migration plan");
    assert!(plan.requires_confirmation());
    assert!(plan.is_full_reindex());
    assert!(plan.is_resumable());
    assert_eq!(
        state.active_model().expect("active model").identity(),
        &original
    );

    state
        .begin_model_migration(plan.confirm())
        .expect("confirmed migration starts");
    state
        .checkpoint_model_migration(57, Some("document-0057".to_owned()))
        .expect("checkpoint progress");
    store.save(&state).expect("persist resumable checkpoint");
    let mut resumed = store
        .load_or_default(&directory.path().join("unused"))
        .expect("reload checkpoint");

    assert_eq!(
        resumed
            .pending_model_migration()
            .expect("pending migration")
            .completed_documents(),
        57
    );
    assert_eq!(
        resumed.active_model().expect("active model").identity(),
        &original
    );
    resumed
        .checkpoint_model_migration(120, None)
        .expect("finish reindex work");
    resumed
        .complete_model_migration()
        .expect("activate only after full reindex");
    assert_eq!(
        resumed.active_model().expect("new active model").identity(),
        import.metadata().identity()
    );
}

struct CountingSource {
    calls: AtomicUsize,
}

impl ArtifactSource for CountingSource {
    fn read(&self, _request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Err(ArtifactSourceError::Unavailable(
            "must not download".to_owned(),
        ))
    }
}

struct FixedFreeSpace(u64);

impl FreeSpaceProbe for FixedFreeSpace {
    fn available_bytes(&self, _path: &Path) -> Result<u64, FreeSpaceError> {
        Ok(self.0)
    }
}

struct AcceptActivation;

impl ActivationProbe for AcceptActivation {
    fn validate(
        &self,
        _artifact: &CatalogArtifact,
        _installed_path: &Path,
    ) -> Result<(), ActivationError> {
        Ok(())
    }
}

#[test]
fn install_requires_consent_and_checks_explicit_free_space_reserve_before_download() {
    let directory = project_temp_dir("low-disk-");
    let catalog = signed_fixture_catalog();
    let offer = catalog
        .installation_offer(
            SemanticProfile::CompactMultilingual,
            &[
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
            ],
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &directory.path().join("app-data/semantic"),
            750_000_000,
        )
        .expect("installation offer");
    let source = CountingSource {
        calls: AtomicUsize::new(0),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        directory.path().join("app-data"),
    );

    let result = manager.install(
        offer.consent(),
        &catalog,
        &InstallEnvironment::new(
            TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            BTreeMap::new(),
        ),
        &source,
        &FixedFreeSpace(1_000_000_000),
        &AcceptActivation,
    );

    assert!(matches!(
        result,
        Err(InstallError::InsufficientSpace { .. })
    ));
    assert_eq!(source.calls.load(Ordering::Relaxed), 0);
    assert!(
        manager
            .state()
            .expect("unchanged state")
            .installed_components()
            .is_empty()
    );
}

struct ResumableMemorySource {
    artifacts: BTreeMap<ArtifactId, Vec<u8>>,
    requests: Mutex<Vec<(ArtifactId, u64)>>,
    interrupt_worker_once: AtomicBool,
}

struct BlockingArtifactSource {
    inner: ResumableMemorySource,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    blocked: AtomicBool,
}

impl ArtifactSource for BlockingArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        if !self.blocked.swap(true, Ordering::AcqRel) {
            self.entered
                .send(())
                .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
            self.release
                .lock()
                .unwrap()
                .recv()
                .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        }
        self.inner.read(request)
    }
}

impl ArtifactSource for ResumableMemorySource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        self.requests
            .lock()
            .expect("requests lock")
            .push((request.artifact_id().clone(), request.offset()));
        if request.artifact_id().as_str() == "fixture.worker.macos-aarch64.1-2-0"
            && request.offset() == 5
            && self.interrupt_worker_once.swap(false, Ordering::AcqRel)
        {
            return Err(ArtifactSourceError::Unavailable(
                "fixture interruption".to_owned(),
            ));
        }
        let bytes = self
            .artifacts
            .get(request.artifact_id())
            .expect("requested signed fixture artifact");
        let start = usize::try_from(request.offset()).expect("fixture offset fits usize");
        let end = (start + 5).min(bytes.len());
        Ok(ArtifactChunk::new(
            bytes[start..end].to_vec(),
            end == bytes.len(),
        ))
    }
}

fn fixture_install_offer(
    catalog: &TrustedCatalog,
    root: &Path,
) -> fm_semantic_components::InstallationOffer {
    catalog
        .installation_offer(
            SemanticProfile::CompactMultilingual,
            &[
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
            ],
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            root,
            100,
        )
        .expect("fixture install offer")
}

fn signed_worker_update_catalog() -> TrustedCatalog {
    signed_worker_patch_catalog(
        "fixture.worker.macos-aarch64.1-2-1",
        "1.2.1",
        b"fixture worker patch",
        "fixture-catalog-worker-patch",
    )
}

fn signed_worker_patch_catalog(
    artifact_id: &str,
    version: &str,
    payload: &[u8],
    revision: &str,
) -> TrustedCatalog {
    let target = TargetTriple::new("macos", "aarch64").unwrap();
    let worker = CatalogArtifact::new(
        ArtifactId::new(artifact_id).unwrap(),
        ComponentId::new("fixture.worker").unwrap(),
        ArtifactKind::Worker,
        Version::parse(version).unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/worker-patch").unwrap(),
        LicenseInfo::new("MIT", "Fixture worker notice").unwrap(),
        Sha256Digest::calculate(payload),
        ComponentResources::new(
            u64::try_from(payload.len()).unwrap(),
            27_000_000,
            48_000_000,
        )
        .unwrap(),
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 2).unwrap()),
            Vec::new(),
            7,
        ),
    )
    .unwrap();
    let runtime = CatalogArtifact::new(
        ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
        ComponentId::new("fixture.runtime").unwrap(),
        ArtifactKind::Runtime,
        Version::parse("1.4.0").unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/runtime").unwrap(),
        LicenseInfo::new("MIT", "Fixture runtime notice").unwrap(),
        Sha256Digest::calculate(b"fixture runtime"),
        ComponentResources::new(15, 44_000_000, 80_000_000).unwrap(),
        ArtifactCompatibility::new(Some(target), None, Vec::new(), 7),
    )
    .unwrap();
    let metadata = fixture_model_metadata();
    let model_artifact_id = ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap();
    let model = CatalogArtifact::new(
        model_artifact_id.clone(),
        ComponentId::new("fixture.model.multilingual").unwrap(),
        ArtifactKind::Model(metadata.identity().clone()),
        Version::parse("1.0.0").unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/model").unwrap(),
        metadata.license().clone(),
        Sha256Digest::calculate(b"fixture model"),
        ComponentResources::new(
            13,
            metadata.estimated_disk_bytes(),
            metadata.estimated_ram_bytes(),
        )
        .unwrap(),
        ArtifactCompatibility::new(None, None, vec![metadata.runtime().clone()], 7),
    )
    .unwrap();
    let model_manifest = ModelManifest::new(model_artifact_id, metadata);
    trust_manifest(
        CatalogManifest::new(
            ManifestRevision::new(revision).unwrap(),
            vec![worker, runtime, model],
            vec![model_manifest.clone()],
            [(
                SemanticProfile::CompactMultilingual,
                model_manifest.metadata().identity().clone(),
            )]
            .into(),
        )
        .unwrap(),
    )
}

fn signed_runtime_replacement_catalog() -> TrustedCatalog {
    let target = TargetTriple::new("macos", "aarch64").unwrap();
    let worker = CatalogArtifact::new(
        ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
        ComponentId::new("fixture.worker").unwrap(),
        ArtifactKind::Worker,
        Version::parse("1.2.0").unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/worker").unwrap(),
        LicenseInfo::new("MIT", "Fixture worker notice").unwrap(),
        Sha256Digest::calculate(b"fixture worker"),
        ComponentResources::new(14, 26_000_000, 48_000_000).unwrap(),
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 2).unwrap()),
            Vec::new(),
            7,
        ),
    )
    .unwrap();
    let runtime_payload = b"fixture runtime replacement";
    let runtime = CatalogArtifact::new(
        ArtifactId::new("fixture.runtime.macos-aarch64.1-4-1").unwrap(),
        ComponentId::new("fixture.runtime").unwrap(),
        ArtifactKind::Runtime,
        Version::parse("1.4.1").unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/runtime-replacement").unwrap(),
        LicenseInfo::new("MIT", "Fixture runtime notice").unwrap(),
        Sha256Digest::calculate(runtime_payload),
        ComponentResources::new(
            u64::try_from(runtime_payload.len()).unwrap(),
            44_000_000,
            80_000_000,
        )
        .unwrap(),
        ArtifactCompatibility::new(Some(target), None, Vec::new(), 7),
    )
    .unwrap();
    let metadata = fixture_model_metadata();
    let model_artifact_id = ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap();
    let model = CatalogArtifact::new(
        model_artifact_id.clone(),
        ComponentId::new("fixture.model.multilingual").unwrap(),
        ArtifactKind::Model(metadata.identity().clone()),
        Version::parse("1.0.0").unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/model").unwrap(),
        metadata.license().clone(),
        Sha256Digest::calculate(b"fixture model"),
        ComponentResources::new(
            13,
            metadata.estimated_disk_bytes(),
            metadata.estimated_ram_bytes(),
        )
        .unwrap(),
        ArtifactCompatibility::new(None, None, vec![metadata.runtime().clone()], 7),
    )
    .unwrap();
    let model_manifest = ModelManifest::new(model_artifact_id, metadata);
    trust_manifest(
        CatalogManifest::new(
            ManifestRevision::new("fixture-catalog-runtime-replacement").unwrap(),
            vec![worker, runtime, model],
            vec![model_manifest.clone()],
            [(
                SemanticProfile::CompactMultilingual,
                model_manifest.metadata().identity().clone(),
            )]
            .into(),
        )
        .unwrap(),
    )
}

fn installed_fixture(prefix: &str) -> (TempDir, ComponentManager, InstallEnvironment) {
    let directory = project_temp_dir(prefix);
    let app_data = directory.path().join("app-data");
    let root = app_data.join("semantic");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                b"fixture worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );
    let environment = InstallEnvironment::new(
        TargetTriple::new("macos", "aarch64").unwrap(),
        1,
        BTreeMap::new(),
    );
    manager
        .install(
            fixture_install_offer(&catalog, &root).consent(),
            &catalog,
            &environment,
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("initial install");
    (directory, manager, environment)
}

#[test]
fn interrupted_install_resumes_by_artifact_id_and_never_activates_partial_content() {
    let directory = project_temp_dir("resume-");
    let app_data = directory.path().join("app-data");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                b"fixture worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(true),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );
    let environment = InstallEnvironment::new(
        TargetTriple::new("macos", "aarch64").unwrap(),
        1,
        BTreeMap::new(),
    );
    let root = app_data.join("semantic");

    let interrupted = manager.install(
        fixture_install_offer(&catalog, &root).consent(),
        &catalog,
        &environment,
        &source,
        &FixedFreeSpace(u64::MAX),
        &AcceptActivation,
    );
    assert!(matches!(
        interrupted,
        Err(InstallError::DownloadInterrupted { .. })
    ));
    assert!(
        manager
            .state()
            .expect("state after interruption")
            .installed_components()
            .is_empty()
    );

    let receipt = manager
        .install(
            fixture_install_offer(&catalog, &root).consent(),
            &catalog,
            &environment,
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("resume and install");

    assert_eq!(receipt.installed_artifacts().len(), 3);
    assert_eq!(
        manager
            .state()
            .expect("installed state")
            .installed_components()
            .len(),
        3
    );
    let worker_offsets: Vec<u64> = source
        .requests
        .lock()
        .expect("requests lock")
        .iter()
        .filter(|(id, _)| id.as_str() == "fixture.worker.macos-aarch64.1-2-0")
        .map(|(_, offset)| *offset)
        .collect();
    assert_eq!(worker_offsets, vec![0, 5, 5, 10]);
}

#[test]
fn near_complete_resume_requires_only_peak_incremental_bytes_plus_reserve() {
    let directory = project_temp_dir("resume-space-");
    let app_data = directory.path().join("app-data");
    let root = SemanticDataRoot::from_app_data(&app_data);
    root.initialize().unwrap();
    let downloads = root.category_path(DataCategory::Catalog).join("downloads");
    std::fs::create_dir(&downloads).unwrap();
    for (artifact, bytes) in [
        (
            "fixture.worker.macos-aarch64.1-2-0",
            b"fixture worke".as_slice(),
        ),
        (
            "fixture.runtime.macos-aarch64.1-4-0",
            b"fixture runtim".as_slice(),
        ),
        (
            "fixture.model.multilingual.deadbeef",
            b"fixture mode".as_slice(),
        ),
    ] {
        std::fs::write(downloads.join(format!("{artifact}.partial")), bytes).unwrap();
    }

    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                b"fixture worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let catalog = signed_fixture_catalog();
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    manager
        .install(
            fixture_install_offer(&catalog, root.path()).consent(),
            &catalog,
            &InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            &source,
            &FixedFreeSpace(350_000_061),
            &AcceptActivation,
        )
        .expect("retained partial bytes count toward the peak footprint");

    let requests = source.requests.lock().unwrap();
    assert!(requests.contains(&(
        ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
        13,
    )));
    assert!(requests.contains(&(
        ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
        14,
    )));
    assert!(requests.contains(&(
        ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
        12,
    )));
}

#[test]
fn concurrent_install_and_move_preserve_both_the_installation_and_new_data_root() {
    let directory = project_temp_dir("install-move-race-");
    let app_data = directory.path().join("app-data");
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );
    let catalog = signed_fixture_catalog();
    let offer = fixture_install_offer(&catalog, &app_data.join("semantic"));
    let environment = InstallEnvironment::new(
        TargetTriple::new("macos", "aarch64").unwrap(),
        1,
        BTreeMap::new(),
    );
    let (download_entered_tx, download_entered_rx) = mpsc::sync_channel(0);
    let (download_release_tx, download_release_rx) = mpsc::sync_channel(0);
    let source = Arc::new(BlockingArtifactSource {
        inner: ResumableMemorySource {
            artifacts: [
                (
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                    b"fixture worker".to_vec(),
                ),
                (
                    ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                    b"fixture runtime".to_vec(),
                ),
                (
                    ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                    b"fixture model".to_vec(),
                ),
            ]
            .into(),
            requests: Mutex::new(Vec::new()),
            interrupt_worker_once: AtomicBool::new(false),
        },
        entered: download_entered_tx,
        release: Mutex::new(download_release_rx),
        blocked: AtomicBool::new(false),
    });
    let install_manager = manager.clone();
    let install_source = Arc::clone(&source);
    let install = thread::spawn(move || {
        install_manager.install(
            offer.consent(),
            &catalog,
            &environment,
            install_source.as_ref(),
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
    });
    download_entered_rx.recv().unwrap();

    let destination = directory.path().join("moved-after-install");
    let move_manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );
    let move_destination = destination.clone();
    let (move_started_tx, move_started_rx) = mpsc::sync_channel(0);
    let (move_done_tx, move_done_rx) = mpsc::sync_channel(1);
    let move_data = thread::spawn(move || {
        move_started_tx.send(()).unwrap();
        let result = move_manager.move_data_root(
            &move_destination,
            &TestIndexingController {
                tracker: Arc::new(PauseTracker {
                    paused: AtomicUsize::new(0),
                    resumed: AtomicUsize::new(0),
                }),
            },
            &NeverCancel,
        );
        move_done_tx.send(()).unwrap();
        result
    });
    move_started_rx.recv().unwrap();
    thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        move_done_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    download_release_tx.send(()).unwrap();
    install.join().unwrap().unwrap();
    move_data.join().unwrap().unwrap();

    let state = manager.state().unwrap();
    assert_eq!(state.data_root().path(), destination);
    assert_eq!(state.installed_components().len(), 3);
    assert!(
        state
            .installed_components()
            .iter()
            .all(|component| component.installed_path().starts_with(&destination))
    );
}

#[test]
fn verified_rename_interruption_resumes_without_redownloading_or_activating_early() {
    let directory = project_temp_dir("rename-resume-");
    let app_data = directory.path().join("app-data");
    let root = SemanticDataRoot::from_app_data(&app_data);
    root.initialize().expect("semantic data layout");
    let orphaned_payload = root
        .category_path(DataCategory::Workers)
        .join("fixture.worker/1.2.0/fixture.worker.macos-aarch64.1-2-0/payload");
    std::fs::create_dir_all(
        orphaned_payload
            .parent()
            .expect("orphaned payload has parent"),
    )
    .expect("create orphaned final directory");
    std::fs::write(&orphaned_payload, b"fixture worker").expect("write verified orphan");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let receipt = manager
        .install(
            fixture_install_offer(&catalog, root.path()).consent(),
            &catalog,
            &InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("recover final rename and finish transaction");

    assert_eq!(receipt.installed_artifacts().len(), 3);
    assert!(
        source
            .requests
            .lock()
            .expect("request log")
            .iter()
            .all(|(id, _)| id.as_str() != "fixture.worker.macos-aarch64.1-2-0")
    );
    assert_eq!(
        manager
            .state()
            .expect("state switched after recovery")
            .installed_components()
            .len(),
        3
    );
}

#[test]
fn staged_install_interruption_recovers_verified_bytes_without_redownloading() {
    let directory = project_temp_dir("staging-resume-");
    let app_data = directory.path().join("app-data");
    let root = SemanticDataRoot::from_app_data(&app_data);
    root.initialize().expect("semantic data layout");
    let staged_payload = root
        .category_path(DataCategory::Workers)
        .join(".staging/fixture.worker.macos-aarch64.1-2-0/payload");
    std::fs::create_dir_all(staged_payload.parent().expect("staged payload has parent"))
        .expect("create deterministic staging directory");
    std::fs::write(&staged_payload, b"fixture worker").expect("write verified staged payload");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    manager
        .install(
            fixture_install_offer(&catalog, root.path()).consent(),
            &catalog,
            &InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("recover staging and finish transaction");

    assert!(
        source
            .requests
            .lock()
            .expect("request log")
            .iter()
            .all(|(id, _)| id.as_str() != "fixture.worker.macos-aarch64.1-2-0")
    );
    assert!(!staged_payload.exists());
}

#[test]
fn successful_worker_update_keeps_the_previous_worker_for_rollback() {
    let (_directory, manager, environment) = installed_fixture("worker-update-");
    let update_catalog = signed_worker_update_catalog();
    let update_source = ResumableMemorySource {
        artifacts: [(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
            b"fixture worker patch".to_vec(),
        )]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let update = update_catalog
        .worker_patch_update(
            &ComponentId::new("fixture.worker").unwrap(),
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .expect("eligible automatic worker patch");
    let receipt = manager
        .install_worker_patch_update(
            update,
            &update_catalog,
            &environment,
            &update_source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("worker patch update");
    let state = manager.state().expect("updated state");
    let worker_id = ComponentId::new("fixture.worker").unwrap();

    assert_eq!(receipt.installed_artifacts().len(), 1);
    assert_eq!(state.active_index_schema_version(), Some(7));
    assert_eq!(
        state
            .installed_component(&worker_id)
            .expect("active worker")
            .version(),
        &Version::parse("1.2.1").unwrap()
    );
    assert_eq!(
        state
            .last_working_worker(&worker_id)
            .expect("rollback worker")
            .version(),
        &Version::parse("1.2.0").unwrap()
    );
}

#[test]
fn repeated_worker_patches_retain_only_the_active_and_single_rollback_payloads() {
    let (_directory, manager, environment) = installed_fixture("worker-update-gc-");
    let worker_id = ComponentId::new("fixture.worker").unwrap();
    let original_path = manager
        .state()
        .unwrap()
        .installed_component(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let first_catalog = signed_worker_update_catalog();
    let first_update = first_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    manager
        .install_worker_patch_update(
            first_update,
            &first_catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
                    b"fixture worker patch".to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .unwrap();
    let first_patch_path = manager
        .state()
        .unwrap()
        .installed_component(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let second_payload = b"fixture worker patch two";
    let second_catalog = signed_worker_patch_catalog(
        "fixture.worker.macos-aarch64.1-2-2",
        "1.2.2",
        second_payload,
        "fixture-catalog-worker-patch-two",
    );
    let second_update = second_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.1").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    manager
        .install_worker_patch_update(
            second_update,
            &second_catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-2").unwrap(),
                    second_payload.to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .unwrap();

    let state = manager.state().unwrap();
    let active = state.installed_component(&worker_id).unwrap();
    let rollback = state.last_working_worker(&worker_id).unwrap();
    assert_eq!(active.version(), &Version::parse("1.2.2").unwrap());
    assert_eq!(rollback.version(), &Version::parse("1.2.1").unwrap());
    assert!(active.installed_path().is_file());
    assert_eq!(rollback.installed_path(), first_patch_path);
    assert!(first_patch_path.is_file());
    assert!(!original_path.exists());
}

#[test]
fn non_worker_replacement_collects_the_displaced_payload_after_commit() {
    let (_directory, manager, environment) = installed_fixture("runtime-replacement-gc-");
    let runtime_id = ComponentId::new("fixture.runtime").unwrap();
    let old_runtime_path = manager
        .state()
        .unwrap()
        .installed_component(&runtime_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let catalog = signed_runtime_replacement_catalog();
    let state = manager.state().unwrap();
    let offer = catalog
        .installation_offer(
            SemanticProfile::CompactMultilingual,
            &[
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-1").unwrap(),
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
            ],
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            state.data_root().path(),
            100,
        )
        .unwrap();

    let receipt = manager
        .install(
            offer.consent(),
            &catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.runtime.macos-aarch64.1-4-1").unwrap(),
                    b"fixture runtime replacement".to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .unwrap();

    let state = manager.state().unwrap();
    let runtime = state.installed_component(&runtime_id).unwrap();
    assert_eq!(runtime.version(), &Version::parse("1.4.1").unwrap());
    assert!(runtime.installed_path().is_file());
    assert!(!old_runtime_path.exists());
    assert!(receipt.cleanup_issues().is_empty());
}

#[test]
fn post_commit_cleanup_failure_is_diagnostic_without_corrupting_component_state() {
    let (directory, _manager, environment) = installed_fixture("runtime-cleanup-failure-");
    let state_directory = directory.path().join("config");
    let app_data = directory.path().join("app-data");
    let runtime_id = ComponentId::new("fixture.runtime").unwrap();
    let default_manager =
        ComponentManager::new(SemanticStateStore::new(&state_directory), &app_data);
    let old_runtime_path = default_manager
        .state()
        .unwrap()
        .installed_component(&runtime_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let durability = Arc::new(FailNextDirectorySync {
        target: old_runtime_path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned(),
        armed: AtomicBool::new(true),
    });
    let manager = ComponentManager::new(
        SemanticStateStore::with_filesystem_durability(
            &state_directory,
            Arc::clone(&durability) as Arc<dyn FilesystemDurability>,
        ),
        &app_data,
    );
    let catalog = signed_runtime_replacement_catalog();
    let state = manager.state().unwrap();
    let offer = catalog
        .installation_offer(
            SemanticProfile::CompactMultilingual,
            &[
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-1").unwrap(),
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
            ],
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            state.data_root().path(),
            100,
        )
        .unwrap();

    let receipt = manager
        .install(
            offer.consent(),
            &catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.runtime.macos-aarch64.1-4-1").unwrap(),
                    b"fixture runtime replacement".to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect("component commit succeeds even when stale cleanup needs retry");

    assert_eq!(receipt.cleanup_issues().len(), 1);
    assert_eq!(
        receipt.cleanup_issues()[0].path(),
        old_runtime_path.parent().unwrap()
    );
    let state = manager.state().expect("committed state remains valid");
    assert_eq!(
        state.installed_component(&runtime_id).unwrap().version(),
        &Version::parse("1.4.1").unwrap()
    );
    assert!(
        state
            .installed_components()
            .iter()
            .all(|component| component.installed_path().is_file())
    );
}

struct FailNextDirectorySync {
    target: PathBuf,
    armed: AtomicBool,
}

impl FilesystemDurability for FailNextDirectorySync {
    fn sync_directory(&self, directory: &Path) -> Result<(), std::io::Error> {
        if directory == self.target && self.armed.swap(false, Ordering::SeqCst) {
            return Err(std::io::Error::other("injected directory sync failure"));
        }
        Ok(())
    }
}

#[test]
fn state_sync_failure_after_worker_rename_restores_recoverable_old_state() {
    let (directory, _manager, environment) = installed_fixture("state-sync-worker-");
    let state_directory = directory.path().join("config");
    let durability = Arc::new(FailNextDirectorySync {
        target: state_directory.clone(),
        armed: AtomicBool::new(false),
    });
    let manager = ComponentManager::new(
        SemanticStateStore::with_filesystem_durability(
            &state_directory,
            Arc::clone(&durability) as Arc<dyn FilesystemDurability>,
        ),
        directory.path().join("app-data"),
    );
    let worker_id = ComponentId::new("fixture.worker").unwrap();
    let old_state = manager.state().unwrap();
    let old_worker = old_state
        .installed_component(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let update_catalog = signed_worker_update_catalog();
    let update = update_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    durability.armed.store(true, Ordering::SeqCst);

    let error = manager
        .install_worker_patch_update(
            update,
            &update_catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
                    b"fixture worker patch".to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect_err("state sync failure must not commit the worker patch");

    assert!(matches!(
        error,
        InstallError::State(SemanticStateError::Io(_))
    ));
    let recovered = manager.state().expect("old state remains readable");
    assert_eq!(
        recovered.installed_component(&worker_id).unwrap().version(),
        &Version::parse("1.2.0").unwrap()
    );
    assert!(old_worker.is_file());
    assert!(
        recovered
            .installed_components()
            .iter()
            .chain(recovered.last_working_worker(&worker_id).into_iter())
            .all(|component| component.installed_path().is_file())
    );
}

#[test]
fn failed_rollback_worker_reinstall_retains_the_referenced_payload_and_valid_state() {
    let (_directory, manager, environment) = installed_fixture("rollback-reinstall-");
    let update_catalog = signed_worker_update_catalog();
    let update_source = ResumableMemorySource {
        artifacts: [(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
            b"fixture worker patch".to_vec(),
        )]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let worker_id = ComponentId::new("fixture.worker").unwrap();
    let update = update_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    manager
        .install_worker_patch_update(
            update,
            &update_catalog,
            &environment,
            &update_source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .unwrap();
    let before = manager.state().unwrap();
    let rollback_path = before
        .last_working_worker(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let rollback_bytes = std::fs::read(&rollback_path).unwrap();
    let original_catalog = signed_fixture_catalog();

    let error = manager
        .install(
            fixture_install_offer(&original_catalog, before.data_root().path()).consent(),
            &original_catalog,
            &environment,
            &CountingSource {
                calls: AtomicUsize::new(0),
            },
            &FixedFreeSpace(0),
            &AcceptActivation,
        )
        .expect_err("low disk must reject the rollback reinstall");

    assert!(matches!(error, InstallError::InsufficientSpace { .. }));
    assert_eq!(std::fs::read(&rollback_path).unwrap(), rollback_bytes);
    let after = manager
        .state()
        .expect("retained rollback state remains valid");
    assert_eq!(
        after
            .last_working_worker(&worker_id)
            .unwrap()
            .installed_path(),
        rollback_path
    );
}

#[test]
fn failed_second_worker_patch_retains_the_existing_rollback_payload() {
    let (_directory, manager, environment) = installed_fixture("rollback-patch-failure-");
    let worker_id = ComponentId::new("fixture.worker").unwrap();
    let first_catalog = signed_worker_update_catalog();
    let first_update = first_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();
    manager
        .install_worker_patch_update(
            first_update,
            &first_catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
                    b"fixture worker patch".to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .unwrap();
    let before = manager.state().unwrap();
    let active_path = before
        .installed_component(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let rollback_path = before
        .last_working_worker(&worker_id)
        .unwrap()
        .installed_path()
        .to_owned();
    let second_payload = b"fixture worker patch two";
    let second_catalog = signed_worker_patch_catalog(
        "fixture.worker.macos-aarch64.1-2-2",
        "1.2.2",
        second_payload,
        "fixture-catalog-worker-patch-two",
    );
    let second_update = second_catalog
        .worker_patch_update(
            &worker_id,
            &Version::parse("1.2.1").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .unwrap();

    let error = manager
        .install_worker_patch_update(
            second_update,
            &second_catalog,
            &environment,
            &ResumableMemorySource {
                artifacts: [(
                    ArtifactId::new("fixture.worker.macos-aarch64.1-2-2").unwrap(),
                    second_payload.to_vec(),
                )]
                .into(),
                requests: Mutex::new(Vec::new()),
                interrupt_worker_once: AtomicBool::new(false),
            },
            &FixedFreeSpace(u64::MAX),
            &RejectWorkerActivation,
        )
        .expect_err("failed patch activation cannot displace rollback state");

    assert!(matches!(error, InstallError::ActivationFailed { .. }));
    let state = manager.state().expect("rollback state remains valid");
    assert_eq!(
        state
            .installed_component(&worker_id)
            .unwrap()
            .installed_path(),
        active_path
    );
    assert_eq!(
        state
            .last_working_worker(&worker_id)
            .unwrap()
            .installed_path(),
        rollback_path
    );
    assert!(active_path.is_file());
    assert!(rollback_path.is_file());
}

struct RejectWorkerActivation;

impl ActivationProbe for RejectWorkerActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        _installed_path: &Path,
    ) -> Result<(), ActivationError> {
        if matches!(artifact.kind(), ArtifactKind::Worker) {
            Err(ActivationError::new("fixture worker did not start"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn failed_worker_update_leaves_the_last_working_worker_active() {
    let (_directory, manager, environment) = installed_fixture("worker-rollback-");
    let update_catalog = signed_worker_update_catalog();
    let update = update_catalog
        .worker_patch_update(
            &ComponentId::new("fixture.worker").unwrap(),
            &Version::parse("1.2.0").unwrap(),
            7,
            &TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            &BTreeMap::new(),
            100,
        )
        .expect("eligible automatic patch");
    let source = ResumableMemorySource {
        artifacts: [(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-1").unwrap(),
            b"fixture worker patch".to_vec(),
        )]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };

    let error = manager
        .install_worker_patch_update(
            update,
            &update_catalog,
            &environment,
            &source,
            &FixedFreeSpace(u64::MAX),
            &RejectWorkerActivation,
        )
        .expect_err("failed candidate must not replace the active worker");

    assert!(matches!(error, InstallError::ActivationFailed { .. }));
    let state = manager.state().expect("state after failed update");
    let active = state
        .installed_component(&ComponentId::new("fixture.worker").unwrap())
        .expect("last working worker remains active");
    assert_eq!(active.version(), &Version::parse("1.2.0").unwrap());
    assert!(active.installed_path().is_file());
}

#[test]
fn tampered_artifact_is_rejected_and_removed_before_activation() {
    let directory = project_temp_dir("tampered-");
    let app_data = directory.path().join("app-data");
    let root = app_data.join("semantic");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
            b"tampered work!".to_vec(),
        )]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let error = manager
        .install(
            fixture_install_offer(&catalog, &root).consent(),
            &catalog,
            &InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect_err("tampered worker must be rejected");

    assert!(matches!(error, InstallError::ChecksumMismatch { .. }));
    assert!(
        manager
            .state()
            .expect("unchanged state")
            .installed_components()
            .is_empty()
    );
    assert!(
        !root
            .join("catalog/downloads/fixture.worker.macos-aarch64.1-2-0.partial")
            .exists()
    );
}

#[test]
fn truncated_artifact_is_rejected_by_the_managed_component_installer() {
    let directory = project_temp_dir("truncated-");
    let app_data = directory.path().join("app-data");
    let root = app_data.join("semantic");
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [(
            ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
            b"short".to_vec(),
        )]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let error = manager
        .install(
            fixture_install_offer(&catalog, &root).consent(),
            &catalog,
            &InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            &source,
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect_err("truncated worker must be rejected");

    assert!(matches!(error, InstallError::ArtifactSizeMismatch { .. }));
}

#[test]
fn unknown_artifact_cannot_enter_a_managed_installation_plan() {
    let catalog = signed_fixture_catalog();
    let unknown = ArtifactId::new("procyon.semantic.worker.unknown").unwrap();

    assert!(matches!(
        catalog.installation_artifacts(SemanticProfile::CompactMultilingual, &[unknown]),
        Err(CatalogError::UnknownArtifact { .. })
    ));
}

#[test]
fn schema_change_requires_confirmation_and_a_resumable_full_reindex() {
    let directory = project_temp_dir("schema-migration-");
    let mut state = SemanticStateStore::new(directory.path().join("config"))
        .load_or_default(&directory.path().join("app-data"))
        .expect("default state");
    state
        .activate_initial_model(
            SemanticProfile::CompactMultilingual,
            fixture_model_metadata().identity().clone(),
            7,
        )
        .expect("initial model");

    let plan = state
        .plan_schema_migration(7, 8, ReindexEstimate::new(42, 4_200_000))
        .expect("schema migration plan");

    assert_eq!(
        plan.reason(),
        ReindexReason::SchemaChanged {
            from_version: 7,
            to_version: 8,
        }
    );
    assert!(plan.requires_confirmation());
    assert!(plan.is_full_reindex());
    assert!(plan.is_resumable());
    assert_eq!(plan.estimate(), ReindexEstimate::new(42, 4_200_000));
    assert!(state.pending_model_migration().is_none());
}

#[test]
fn installation_rejects_an_active_schema_change_before_mutating_state_or_artifacts() {
    let (directory, manager, environment) = installed_fixture("schema-install-guard-");
    let state_path = directory.path().join("config/semantic-components.json");
    let state_before = std::fs::read(&state_path).unwrap();
    let state = manager.state().unwrap();
    let payloads: Vec<_> = state
        .installed_components()
        .iter()
        .map(|component| {
            (
                component.installed_path().to_owned(),
                std::fs::read(component.installed_path()).unwrap(),
            )
        })
        .collect();
    let schema_eight_catalog = signed_fixture_catalog_with_schema(8);

    let error = manager
        .install(
            fixture_install_offer(&schema_eight_catalog, state.data_root().path()).consent(),
            &schema_eight_catalog,
            &environment,
            &CountingSource {
                calls: AtomicUsize::new(0),
            },
            &FixedFreeSpace(u64::MAX),
            &AcceptActivation,
        )
        .expect_err("an active index schema cannot change during installation");

    assert!(matches!(
        error,
        InstallError::State(SemanticStateError::IndexSchemaMigrationRequired {
            active_version: 7,
            offered_version: 8
        })
    ));
    assert_eq!(std::fs::read(state_path).unwrap(), state_before);
    assert!(
        payloads
            .iter()
            .all(|(path, bytes)| std::fs::read(path).is_ok_and(|current| current == *bytes))
    );
    assert_eq!(
        manager.state().unwrap().active_index_schema_version(),
        Some(7)
    );
}

struct PauseTracker {
    paused: AtomicUsize,
    resumed: AtomicUsize,
}

struct TestIndexingController {
    tracker: Arc<PauseTracker>,
}

struct TestPauseGuard {
    tracker: Arc<PauseTracker>,
}

struct TestQuiescer {
    calls: Arc<AtomicUsize>,
}

impl ComponentQuiescer for TestQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

fn test_quiescer() -> TestQuiescer {
    TestQuiescer {
        calls: Arc::new(AtomicUsize::new(0)),
    }
}

struct RejectQuiescer;

impl ComponentQuiescer for RejectQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        Err(QuiesceError::new("worker is still active"))
    }
}

impl Drop for TestPauseGuard {
    fn drop(&mut self) {
        self.tracker.resumed.fetch_add(1, Ordering::Relaxed);
    }
}

impl IndexingPauseGuard for TestPauseGuard {}

impl IndexingController for TestIndexingController {
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError> {
        self.tracker.paused.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(TestPauseGuard {
            tracker: Arc::clone(&self.tracker),
        }))
    }
}

struct BlockingIndexingController {
    tracker: Arc<PauseTracker>,
    entered: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl IndexingController for BlockingIndexingController {
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError> {
        self.tracker.paused.fetch_add(1, Ordering::Relaxed);
        self.entered
            .send(())
            .map_err(|error| PauseError::new(error.to_string()))?;
        self.release
            .lock()
            .unwrap()
            .recv()
            .map_err(|error| PauseError::new(error.to_string()))?;
        Ok(Box::new(TestPauseGuard {
            tracker: Arc::clone(&self.tracker),
        }))
    }
}

struct NeverCancel;

impl DataMigrationCancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[test]
fn moving_data_root_pauses_copies_known_categories_verifies_and_switches_atomically() {
    let directory = project_temp_dir("move-data-");
    let app_data = directory.path().join("app-data");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let source_root = SemanticDataRoot::from_app_data(&app_data);
    source_root.initialize().expect("source layout");
    for category in DataCategory::all() {
        let path = source_root
            .category_path(*category)
            .join(format!("{}.bin", category.directory_name()));
        std::fs::write(path, category.directory_name().as_bytes()).expect("write category fixture");
    }

    std::fs::write(source_root.path().join("unknown-root-entry"), b"must stay")
        .expect("write unknown source entry");
    store
        .save(
            &store
                .load_or_default(&app_data)
                .expect("default semantic state"),
        )
        .expect("persist initial root");
    let manager = ComponentManager::new(store, &app_data);
    let destination = directory.path().join("custom/semantic-data");
    let tracker = Arc::new(PauseTracker {
        paused: AtomicUsize::new(0),
        resumed: AtomicUsize::new(0),
    });

    let receipt = manager
        .move_data_root(
            &destination,
            &TestIndexingController {
                tracker: Arc::clone(&tracker),
            },
            &NeverCancel,
        )
        .expect("verified data-root migration");

    assert_eq!(receipt.source(), source_root.path());
    assert_eq!(receipt.destination(), destination);
    assert!(receipt.verified_file_count() >= DataCategory::all().len() as u64);
    assert_eq!(
        manager.state().expect("switched state").data_root().path(),
        destination
    );
    for category in DataCategory::all() {
        let relative = format!(
            "{}/{}.bin",
            category.directory_name(),
            category.directory_name()
        );
        assert_eq!(
            std::fs::read(source_root.path().join(&relative)).expect("source retained"),
            std::fs::read(destination.join(relative)).expect("destination verified")
        );
    }
    assert!(!destination.join("unknown-root-entry").exists());
    assert_eq!(tracker.paused.load(Ordering::Relaxed), 1);
    assert_eq!(tracker.resumed.load(Ordering::Relaxed), 1);
}

#[test]
fn state_sync_failure_after_data_root_rename_restores_the_old_root() {
    let (directory, _manager, _environment) = installed_fixture("state-sync-data-root-");
    let state_directory = directory.path().join("config");
    let durability = Arc::new(FailNextDirectorySync {
        target: state_directory.clone(),
        armed: AtomicBool::new(false),
    });
    let manager = ComponentManager::new(
        SemanticStateStore::with_filesystem_durability(
            &state_directory,
            Arc::clone(&durability) as Arc<dyn FilesystemDurability>,
        ),
        directory.path().join("app-data"),
    );
    let before = manager.state().unwrap();
    let source = before.data_root().path().to_owned();
    let source_payloads: Vec<_> = before
        .installed_components()
        .iter()
        .map(|component| component.installed_path().to_owned())
        .collect();
    let destination = directory.path().join("failed-move/semantic-data");
    durability.armed.store(true, Ordering::SeqCst);

    let error = manager
        .move_data_root(
            &destination,
            &TestIndexingController {
                tracker: Arc::new(PauseTracker {
                    paused: AtomicUsize::new(0),
                    resumed: AtomicUsize::new(0),
                }),
            },
            &NeverCancel,
        )
        .expect_err("state sync failure must reject the data-root switch");

    assert!(matches!(
        error,
        fm_semantic_components::DataRootMigrationError::State(SemanticStateError::Io(_))
    ));
    let recovered = manager.state().expect("old data root remains readable");
    assert_eq!(recovered.data_root().path(), source);
    assert!(source_payloads.iter().all(|path| path.is_file()));
    assert!(!destination.exists());
}

#[test]
fn concurrent_move_checkpoint_and_uninstall_do_not_lose_durable_state() {
    let (directory, manager, _environment) = installed_fixture("lifecycle-race-");
    let target = ModelIdentity::new(
        ModelId::new("race-target").unwrap(),
        ModelRevision::new("race-revision").unwrap(),
    );
    let plan = manager
        .plan_model_migration(
            SemanticProfile::CompactEnglish,
            target,
            ReindexEstimate::new(4, 40),
        )
        .unwrap();
    let migration_id = plan.id();
    manager.begin_model_migration(plan.confirm()).unwrap();
    let destination = directory.path().join("race-destination");
    let (move_entered_tx, move_entered_rx) = mpsc::sync_channel(0);
    let (move_release_tx, move_release_rx) = mpsc::sync_channel(0);
    let move_manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        directory.path().join("app-data"),
    );
    let move_destination = destination.clone();
    let move_thread = thread::spawn(move || {
        move_manager.move_data_root(
            &move_destination,
            &BlockingIndexingController {
                tracker: Arc::new(PauseTracker {
                    paused: AtomicUsize::new(0),
                    resumed: AtomicUsize::new(0),
                }),
                entered: move_entered_tx,
                release: Mutex::new(move_release_rx),
            },
            &NeverCancel,
        )
    });
    move_entered_rx.recv().unwrap();

    let checkpoint_manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        directory.path().join("app-data"),
    );
    let (checkpoint_done_tx, checkpoint_done_rx) = mpsc::sync_channel(1);
    let checkpoint_thread = thread::spawn(move || {
        let result =
            checkpoint_manager.checkpoint_model_migration(migration_id, 2, Some("two".to_owned()));
        checkpoint_done_tx.send(()).unwrap();
        result
    });
    let uninstall_manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        directory.path().join("app-data"),
    );
    let (uninstall_done_tx, uninstall_done_rx) = mpsc::sync_channel(1);
    let uninstall_thread = thread::spawn(move || {
        let result = uninstall_manager.uninstall(UninstallIndexDecision::Retain, &test_quiescer());
        uninstall_done_tx.send(()).unwrap();
        result
    });
    thread::sleep(Duration::from_millis(25));
    assert!(matches!(
        checkpoint_done_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        uninstall_done_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    move_release_tx.send(()).unwrap();
    move_thread.join().unwrap().unwrap();
    checkpoint_thread.join().unwrap().unwrap();
    assert!(matches!(
        uninstall_thread.join().unwrap(),
        Err(fm_semantic_components::UninstallError::MigrationInProgress)
    ));

    let state = manager.state().unwrap();
    assert_eq!(state.data_root().path(), destination);
    assert_eq!(
        state
            .pending_model_migration()
            .unwrap()
            .completed_documents(),
        2
    );
    assert_eq!(state.installed_components().len(), 3);
}

struct CancelAfter {
    checks: AtomicUsize,
    allowed_checks: usize,
}

impl DataMigrationCancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.checks.fetch_add(1, Ordering::Relaxed) >= self.allowed_checks
    }
}

#[test]
fn interrupted_data_root_migration_rolls_back_and_retains_the_source() {
    let directory = project_temp_dir("move-cancel-");
    let app_data = directory.path().join("app-data");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let source_root = SemanticDataRoot::from_app_data(&app_data);
    source_root.initialize().expect("source layout");
    let source_file = source_root
        .category_path(DataCategory::Extracted)
        .join("large.bin");
    std::fs::write(&source_file, vec![42_u8; 128 * 1024]).expect("write source fixture");
    let state = store.load_or_default(&app_data).expect("default state");
    store.save(&state).expect("persist initial state");
    let manager = ComponentManager::new(store, &app_data);
    let destination = directory.path().join("cancelled/semantic-data");
    let tracker = Arc::new(PauseTracker {
        paused: AtomicUsize::new(0),
        resumed: AtomicUsize::new(0),
    });

    let result = manager.move_data_root(
        &destination,
        &TestIndexingController {
            tracker: Arc::clone(&tracker),
        },
        &CancelAfter {
            checks: AtomicUsize::new(0),
            allowed_checks: 4,
        },
    );

    assert!(matches!(
        result,
        Err(fm_semantic_components::DataRootMigrationError::Cancelled)
    ));
    assert_eq!(
        manager
            .state()
            .expect("state rolled back")
            .data_root()
            .path(),
        source_root.path()
    );
    assert!(source_file.is_file());
    assert!(!destination.exists());
    assert_eq!(tracker.paused.load(Ordering::Relaxed), 1);
    assert_eq!(tracker.resumed.load(Ordering::Relaxed), 1);
}

#[cfg(unix)]
#[test]
fn data_root_move_rejects_a_symlink_destination_parent_without_writing_through_it() {
    use std::os::unix::fs::symlink;

    let directory = project_temp_dir("move-parent-symlink-");
    let app_data = directory.path().join("app-data");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let source_root = SemanticDataRoot::from_app_data(&app_data);
    source_root.initialize().unwrap();
    std::fs::write(
        source_root
            .category_path(DataCategory::Models)
            .join("model.bin"),
        b"source",
    )
    .unwrap();
    store
        .save(&store.load_or_default(&app_data).unwrap())
        .unwrap();
    let manager = ComponentManager::new(store, &app_data);
    let outside = directory.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let linked_parent = directory.path().join("linked-parent");
    symlink(&outside, &linked_parent).unwrap();

    let result = manager.move_data_root(
        &linked_parent.join("semantic-data"),
        &TestIndexingController {
            tracker: Arc::new(PauseTracker {
                paused: AtomicUsize::new(0),
                resumed: AtomicUsize::new(0),
            }),
        },
        &NeverCancel,
    );

    assert!(matches!(
        result,
        Err(fm_semantic_components::DataRootMigrationError::UnsafeEntry { .. })
    ));
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn data_root_move_detects_overlap_through_an_ancestor_symlink() {
    use std::os::unix::fs::symlink;

    let directory = project_temp_dir("move-overlap-symlink-");
    let app_data = directory.path().join("app-data");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let source_root = SemanticDataRoot::from_app_data(&app_data);
    source_root.initialize().unwrap();
    store
        .save(&store.load_or_default(&app_data).unwrap())
        .unwrap();
    let manager = ComponentManager::new(store, &app_data);
    let alias = directory.path().join("app-data-alias");
    symlink(&app_data, &alias).unwrap();

    let result = manager.move_data_root(
        &alias.join("semantic/nested"),
        &TestIndexingController {
            tracker: Arc::new(PauseTracker {
                paused: AtomicUsize::new(0),
                resumed: AtomicUsize::new(0),
            }),
        },
        &NeverCancel,
    );

    assert!(matches!(
        result,
        Err(fm_semantic_components::DataRootMigrationError::OverlappingRoots)
    ));
    assert!(!source_root.path().join("nested").exists());
}

#[cfg(unix)]
#[test]
fn install_rejects_a_symlinked_component_parent_without_writing_outside_the_data_root() {
    use std::os::unix::fs::symlink;

    let directory = project_temp_dir("install-parent-symlink-");
    let app_data = directory.path().join("app-data");
    let root = SemanticDataRoot::from_app_data(&app_data);
    root.initialize().unwrap();
    let outside = directory.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(
        &outside,
        root.category_path(DataCategory::Workers)
            .join("fixture.worker"),
    )
    .unwrap();
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: [
            (
                ArtifactId::new("fixture.worker.macos-aarch64.1-2-0").unwrap(),
                b"fixture worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture.runtime.macos-aarch64.1-4-0").unwrap(),
                b"fixture runtime".to_vec(),
            ),
            (
                ArtifactId::new("fixture.model.multilingual.deadbeef").unwrap(),
                b"fixture model".to_vec(),
            ),
        ]
        .into(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let result = manager.install(
        fixture_install_offer(&catalog, root.path()).consent(),
        &catalog,
        &InstallEnvironment::new(
            TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            BTreeMap::new(),
        ),
        &source,
        &FixedFreeSpace(u64::MAX),
        &AcceptActivation,
    );

    assert!(result.is_err());
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn install_rejects_a_symlinked_partial_without_overwriting_its_target() {
    use std::os::unix::fs::symlink;

    let directory = project_temp_dir("install-partial-symlink-");
    let app_data = directory.path().join("app-data");
    let root = SemanticDataRoot::from_app_data(&app_data);
    root.initialize().unwrap();
    let downloads = root.category_path(DataCategory::Catalog).join("downloads");
    std::fs::create_dir(&downloads).unwrap();
    let outside = directory.path().join("outside-payload");
    std::fs::write(&outside, b"do not overwrite").unwrap();
    symlink(
        &outside,
        downloads.join("fixture.worker.macos-aarch64.1-2-0.partial"),
    )
    .unwrap();
    let catalog = signed_fixture_catalog();
    let source = ResumableMemorySource {
        artifacts: BTreeMap::new(),
        requests: Mutex::new(Vec::new()),
        interrupt_worker_once: AtomicBool::new(false),
    };
    let manager = ComponentManager::new(
        SemanticStateStore::new(directory.path().join("config")),
        &app_data,
    );

    let result = manager.install(
        fixture_install_offer(&catalog, root.path()).consent(),
        &catalog,
        &InstallEnvironment::new(
            TargetTriple::new("macos", "aarch64").unwrap(),
            1,
            BTreeMap::new(),
        ),
        &source,
        &FixedFreeSpace(u64::MAX),
        &AcceptActivation,
    );

    assert!(matches!(result, Err(InstallError::UnsafePath { .. })));
    assert_eq!(std::fs::read(&outside).unwrap(), b"do not overwrite");
}

#[cfg(unix)]
#[test]
fn data_root_move_preserves_safe_unix_permissions_and_executable_bits() {
    use std::os::unix::fs::PermissionsExt;

    let directory = project_temp_dir("move-permissions-");
    let app_data = directory.path().join("app-data");
    let store = SemanticStateStore::new(directory.path().join("config"));
    let source_root = SemanticDataRoot::from_app_data(&app_data);
    source_root.initialize().unwrap();
    let source_directory = source_root
        .category_path(DataCategory::Workers)
        .join("worker");
    std::fs::create_dir(&source_directory).unwrap();
    let source_executable = source_directory.join("worker-bin");
    std::fs::write(&source_executable, b"worker").unwrap();
    std::fs::set_permissions(&source_directory, std::fs::Permissions::from_mode(0o750)).unwrap();
    std::fs::set_permissions(&source_executable, std::fs::Permissions::from_mode(0o4755)).unwrap();
    store
        .save(&store.load_or_default(&app_data).unwrap())
        .unwrap();
    let manager = ComponentManager::new(store, &app_data);
    let destination = directory.path().join("moved/semantic-data");

    manager
        .move_data_root(
            &destination,
            &TestIndexingController {
                tracker: Arc::new(PauseTracker {
                    paused: AtomicUsize::new(0),
                    resumed: AtomicUsize::new(0),
                }),
            },
            &NeverCancel,
        )
        .unwrap();

    let moved_directory = destination.join("workers/worker");
    let moved_executable = moved_directory.join("worker-bin");
    assert_eq!(
        std::fs::metadata(moved_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
    assert_eq!(
        std::fs::metadata(moved_executable)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn data_root_switch_rebinds_installed_component_paths_to_verified_copies() {
    let (directory, manager, _environment) = installed_fixture("move-installed-");
    let source = manager
        .state()
        .expect("installed state")
        .data_root()
        .clone();
    let destination = directory.path().join("moved/semantic-data");
    let tracker = Arc::new(PauseTracker {
        paused: AtomicUsize::new(0),
        resumed: AtomicUsize::new(0),
    });

    manager
        .move_data_root(
            &destination,
            &TestIndexingController { tracker },
            &NeverCancel,
        )
        .expect("move installed components");
    let state = manager.state().expect("moved state");

    assert!(state.installed_components().iter().all(|component| {
        component.installed_path().starts_with(&destination) && component.installed_path().is_file()
    }));
    assert!(
        source
            .category_path(DataCategory::Workers)
            .join("fixture.worker/1.2.0/fixture.worker.macos-aarch64.1-2-0/payload")
            .is_file()
    );
}

#[test]
fn status_reports_components_and_actual_disk_use_for_every_category() {
    let (_directory, manager, _environment) = installed_fixture("status-");

    let report = manager.status().expect("component status report");

    assert_eq!(report.components().len(), 3);
    assert!(report.components().iter().all(|component| {
        component.status() == ComponentLifecycleStatus::Active && component.installed_bytes() > 0
    }));
    assert_eq!(
        report.disk_use().categories().len(),
        DataCategory::all().len()
    );
    assert_eq!(
        report
            .disk_use()
            .bytes_for(DataCategory::Models)
            .expect("model disk category"),
        13
    );
    assert_eq!(
        report
            .disk_use()
            .bytes_for(DataCategory::Workers)
            .expect("worker disk category"),
        29
    );
    assert_eq!(report.disk_use().total_bytes(), 42);
}

#[test]
fn persisted_component_paths_cannot_escape_the_semantic_data_root() {
    let (directory, manager, _environment) = installed_fixture("state-path-confinement-");
    let outside = directory.path().join("outside-payload");
    std::fs::write(&outside, b"outside").unwrap();
    let state_path = directory.path().join("config/semantic-components.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    state["installed_components"][0]["installed_path"] =
        outside.to_string_lossy().into_owned().into();
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    assert!(manager.status().is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
}

#[test]
fn persisted_state_reports_a_missing_referenced_component_payload() {
    let (_directory, manager, _environment) = installed_fixture("state-missing-payload-");
    let state = manager.state().unwrap();
    let component = state.installed_components().first().unwrap();
    let artifact = component.artifact_id().clone();
    let missing_path = component.installed_path().to_owned();
    std::fs::remove_file(&missing_path).unwrap();

    assert!(matches!(
        manager.state(),
        Err(InstallError::State(
            SemanticStateError::ReferencedPayloadUnavailable {
                artifact: actual_artifact,
                path,
            }
        )) if actual_artifact == artifact && path == missing_path
    ));
}

#[test]
fn uninstall_requires_an_explicit_retain_or_delete_indexes_decision() {
    let (_retain_directory, retain_manager, _environment) = installed_fixture("uninstall-retain-");
    let retain_root = retain_manager
        .state()
        .expect("installed state")
        .data_root()
        .clone();
    let retained_index = retain_root
        .category_path(DataCategory::Zvec)
        .join("index.bin");
    std::fs::write(&retained_index, b"retained index").expect("write index fixture");
    let quiesce_calls = Arc::new(AtomicUsize::new(0));

    let retained = retain_manager
        .uninstall(
            UninstallIndexDecision::Retain,
            &TestQuiescer {
                calls: Arc::clone(&quiesce_calls),
            },
        )
        .expect("uninstall and retain index");
    assert_eq!(retained.index_decision(), UninstallIndexDecision::Retain);
    assert_eq!(retained.removed_component_count(), 3);
    assert!(retained_index.is_file());
    assert!(
        retain_manager
            .state()
            .expect("uninstalled state")
            .installed_components()
            .is_empty()
    );
    assert_eq!(quiesce_calls.load(Ordering::Relaxed), 1);

    let (_delete_directory, delete_manager, _environment) = installed_fixture("uninstall-delete-");
    let delete_root = delete_manager
        .state()
        .expect("installed state")
        .data_root()
        .clone();
    let deleted_index = delete_root
        .category_path(DataCategory::Zvec)
        .join("index.bin");
    std::fs::write(&deleted_index, b"deleted index").expect("write index fixture");

    let deleted = delete_manager
        .uninstall(UninstallIndexDecision::Delete, &test_quiescer())
        .expect("uninstall and delete index");
    assert_eq!(deleted.index_decision(), UninstallIndexDecision::Delete);
    assert!(!deleted_index.exists());
}

#[test]
fn uninstall_does_not_mutate_components_when_workers_cannot_be_quiesced() {
    let (_directory, manager, _environment) = installed_fixture("uninstall-quiesce-failure-");

    let result = manager.uninstall(UninstallIndexDecision::Retain, &RejectQuiescer);

    assert!(matches!(
        result,
        Err(fm_semantic_components::UninstallError::Quiesce(_))
    ));
    assert_eq!(manager.state().unwrap().installed_components().len(), 3);
}

#[test]
fn uninstall_rejects_an_active_model_migration_without_losing_its_checkpoint() {
    let (_directory, manager, _environment) = installed_fixture("uninstall-migration-");
    let target = ModelIdentity::new(
        ModelId::new("replacement.model").unwrap(),
        ModelRevision::new("replacement-revision").unwrap(),
    );
    let plan = manager
        .plan_model_migration(
            SemanticProfile::CompactEnglish,
            target,
            ReindexEstimate::new(4, 40),
        )
        .unwrap();
    let migration_id = plan.id();
    manager.begin_model_migration(plan.confirm()).unwrap();
    manager
        .checkpoint_model_migration(migration_id, 2, Some("two".to_owned()))
        .unwrap();

    let result = manager.uninstall(UninstallIndexDecision::Retain, &test_quiescer());

    assert!(matches!(
        result,
        Err(fm_semantic_components::UninstallError::MigrationInProgress)
    ));
    let pending = manager
        .state()
        .unwrap()
        .pending_model_migration()
        .cloned()
        .unwrap();
    assert_eq!(pending.completed_documents(), 2);
    assert_eq!(pending.resume_cursor(), Some("two"));
}

#[test]
fn status_finishes_an_interrupted_uninstall_after_the_state_switch() {
    let (directory, manager, _environment) = installed_fixture("uninstall-recovery-");
    let config = directory.path().join("config");
    let state_path = config.join("semantic-components.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    let mut target = original.clone();
    target["installed_components"] = serde_json::json!([]);
    target["last_working_workers"] = serde_json::json!([]);
    target["pending_model_migration"] = serde_json::Value::Null;
    let transaction_id = uuid::Uuid::new_v4();
    let root = manager.state().unwrap().data_root().path().to_owned();
    let tombstone = root
        .join("catalog")
        .join(format!(".uninstall-{transaction_id}"));
    std::fs::create_dir(&tombstone).unwrap();
    for category in [DataCategory::Models, DataCategory::Workers] {
        std::fs::rename(
            root.join(category.directory_name()),
            tombstone.join(category.directory_name()),
        )
        .unwrap();
        std::fs::create_dir(root.join(category.directory_name())).unwrap();
    }
    std::fs::write(&state_path, serde_json::to_vec_pretty(&target).unwrap()).unwrap();
    let journal = serde_json::json!({
        "format_version": 1,
        "transaction_id": transaction_id,
        "delete_indexes": false,
        "original_state": original,
        "target_state": target,
        "staged_categories": ["models", "workers"],
    });
    let journal_path = config.join("semantic-uninstall.json");
    std::fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    let status = manager
        .status()
        .expect("status recovers committed uninstall");

    assert!(status.components().is_empty());
    assert!(!journal_path.exists());
    assert!(!tombstone.exists());
}

#[test]
fn status_rolls_back_an_interrupted_uninstall_before_the_state_switch() {
    let (directory, manager, _environment) = installed_fixture("uninstall-rollback-");
    let config = directory.path().join("config");
    let state_path = config.join("semantic-components.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    let mut target = original.clone();
    target["installed_components"] = serde_json::json!([]);
    target["last_working_workers"] = serde_json::json!([]);
    target["pending_model_migration"] = serde_json::Value::Null;
    let transaction_id = uuid::Uuid::new_v4();
    let root = manager.state().unwrap().data_root().path().to_owned();
    let tombstone = root
        .join("catalog")
        .join(format!(".uninstall-{transaction_id}"));
    std::fs::create_dir(&tombstone).unwrap();
    std::fs::rename(root.join("models"), tombstone.join("models")).unwrap();
    let journal = serde_json::json!({
        "format_version": 1,
        "transaction_id": transaction_id,
        "delete_indexes": false,
        "original_state": original,
        "target_state": target,
        "staged_categories": ["models"],
    });
    let journal_path = config.join("semantic-uninstall.json");
    std::fs::write(&journal_path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();

    let status = manager
        .status()
        .expect("status rolls back uncommitted uninstall");

    assert_eq!(status.components().len(), 3);
    assert!(root.join("models").is_dir());
    assert!(!journal_path.exists());
    assert!(!tombstone.exists());
}

#[test]
fn enrolment_deletion_plan_and_result_always_cover_saved_conversation_evidence() {
    let plan = EnrolmentDeletionPlan::new(
        EnrolmentId::new("fixture-enrolment-42").expect("valid enrolment id"),
        EnrolmentDeletionCounts {
            index_records: 14,
            extracted_files: 9,
            zvec_vectors: 14,
            cache_entries: 5,
            conversation_evidence: 3,
        },
    );

    assert_eq!(
        plan.targets(),
        &[
            DeletionTarget::Index,
            DeletionTarget::Extracted,
            DeletionTarget::Zvec,
            DeletionTarget::EmbeddingCache,
            DeletionTarget::ConversationEvidence,
        ]
    );
    assert!(plan.requires_conversation_evidence_deletion());

    let result = plan
        .complete(EnrolmentDeletionCounts {
            index_records: 14,
            extracted_files: 9,
            zvec_vectors: 14,
            cache_entries: 5,
            conversation_evidence: 3,
        })
        .expect("all enrolment-derived data deleted");
    assert_eq!(result.deleted_targets(), plan.targets());
    assert_eq!(result.deleted().conversation_evidence, 3);
    assert!(result.conversation_evidence_deleted());
}
