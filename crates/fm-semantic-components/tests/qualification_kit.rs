//! Public acceptance tests for the private macOS qualification kit.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use ed25519_dalek::SigningKey;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use fm_semantic_components::{
    ArtifactCompatibility, ArtifactId, ArtifactKind, ArtifactLocation, CatalogArtifact,
    CatalogManifest, ComponentId, ComponentResources, EmbeddingNormalization, LicenseInfo,
    ManifestRevision, ModelId, ModelIdentity, ModelManifest, ModelMetadata, ModelPackKind,
    ModelPackSpec, ModelRevision, PRODUCTION_MODEL_COMPONENT_ID,
    PRODUCTION_ONNX_RUNTIME_COMPONENT_ID, PRODUCTION_TOKENIZER_ID, PRODUCTION_WORKER_COMPONENT_ID,
    PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID, ProductionArtifactProvenance, ProductionCatalogManifest,
    ProtocolRange, QualificationInstallError, RuntimeCompatibility, SemanticProfile, Sha256Digest,
    TargetTriple, TokenizerId, install_macos_qualification, production_pipeline_identity,
    sign_production_catalog, write_model_pack, write_signed_production_catalog,
};
use fm_semantic_components::{QualificationProfile, QualificationProfileError};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use semver::{Version, VersionReq};
use tempfile::TempDir;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/component-tests");
    fs::create_dir_all(&parent).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent.canonicalize().unwrap())
        .unwrap()
}

#[test]
fn cleanup_removes_only_a_marker_owned_profile_beneath_the_dedicated_home() {
    let root = project_temp_dir("qualification-cleanup-");
    let home = root.path().join("dedicated-user");
    let profile = home.join("qualification-profiles").join("semantic-alpha");
    fs::create_dir_all(&home).unwrap();
    let qualification = QualificationProfile::create(&home, &profile).unwrap();
    fs::write(
        qualification
            .application_data()
            .join("qualification-evidence"),
        b"private",
    )
    .unwrap();
    let sibling = home.join("ordinary-data");
    fs::write(&sibling, b"keep").unwrap();

    qualification.cleanup().unwrap();

    assert!(!profile.exists());
    assert_eq!(fs::read(sibling).unwrap(), b"keep");
}

#[test]
fn cleanup_rejects_home_normal_app_data_symlinks_and_unowned_profiles() {
    let root = project_temp_dir("qualification-rejections-");
    let home = root.path().join("dedicated-user");
    fs::create_dir_all(home.join("Library/Application Support/fm")).unwrap();

    for unsafe_path in [
        home.clone(),
        home.join("Library"),
        home.join("Library/Application Support/fm"),
        home.join("Library/Application Support/fm/nested-profile"),
        home.join("single-broad-directory"),
        root.path().join("outside"),
    ] {
        assert!(matches!(
            QualificationProfile::create(&home, &unsafe_path),
            Err(QualificationProfileError::UnsafeProfilePath { .. })
        ));
    }

    let unowned = home.join("qualification-profiles").join("existing");
    fs::create_dir_all(&unowned).unwrap();
    assert!(matches!(
        QualificationProfile::create(&home, &unowned),
        Err(QualificationProfileError::PreExistingUnownedProfile { .. })
    ));

    #[cfg(unix)]
    {
        let destination = home.join("destination");
        let linked = home.join("qualification-profiles").join("linked");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        fs::create_dir_all(&destination).unwrap();
        std::os::unix::fs::symlink(destination, &linked).unwrap();
        assert!(matches!(
            QualificationProfile::create(&home, &linked),
            Err(QualificationProfileError::UnsafeProfilePath { .. })
        ));
    }
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_uses_signed_exact_macos_payloads_and_release_layout() {
    let root = project_temp_dir("qualification-install-");
    let home = root.path().join("dedicated-user");
    fs::create_dir_all(&home).unwrap();
    let inputs = qualification_inputs(root.path(), "macos", "aarch64");
    let profile = home.join("qualification-profiles/semantic-alpha");

    let receipt = install_macos_qualification(
        &home,
        &profile,
        &inputs.catalog.join("catalog.json"),
        &inputs.catalog.join("catalog.sig"),
        &inputs.artifacts,
        &inputs.public_key,
    )
    .unwrap();

    assert_eq!(receipt.installed_artifact_count(), 3);
    assert_eq!(
        receipt.application_data(),
        profile.join("Library/Application Support/fm")
    );
    assert!(
        receipt
            .application_data()
            .join("semantic-components/semantic-components.json")
            .is_file()
    );
    let semantic_data = receipt.application_data().join("semantic");
    assert!(semantic_data.is_dir());
    assert!(
        semantic_data
            .join("workers")
            .join(PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID)
            .join("1.0.0/production-runtime/libzvec_c_api.dylib")
            .is_file()
    );
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn install_rejects_signature_payload_set_checksum_and_target_drift() {
    let root = project_temp_dir("qualification-integrity-");
    let home = root.path().join("dedicated-user");
    fs::create_dir_all(&home).unwrap();

    for (case, mutate) in [
        ("signature", mutate_signature as fn(&QualificationInputs)),
        ("checksum", mutate_checksum),
        ("extra", add_extra_payload),
        ("symlink", replace_payload_with_symlink),
    ] {
        let case_root = root.path().join(case);
        fs::create_dir(&case_root).unwrap();
        let inputs = qualification_inputs(&case_root, "macos", "aarch64");
        mutate(&inputs);
        let profile = home.join(format!("qualification-profiles/{case}"));
        assert!(
            install_macos_qualification(
                &home,
                &profile,
                &inputs.catalog.join("catalog.json"),
                &inputs.catalog.join("catalog.sig"),
                &inputs.artifacts,
                &inputs.public_key,
            )
            .is_err()
        );
        assert!(!profile.exists(), "{case} left a partial profile");
    }

    let target_root = root.path().join("target");
    fs::create_dir(&target_root).unwrap();
    let inputs = qualification_inputs(&target_root, "linux", "aarch64");
    let profile = home.join("qualification-profiles/target");
    assert!(matches!(
        install_macos_qualification(
            &home,
            &profile,
            &inputs.catalog.join("catalog.json"),
            &inputs.catalog.join("catalog.sig"),
            &inputs.artifacts,
            &inputs.public_key,
        ),
        Err(QualificationInstallError::WrongTarget)
    ));
    assert!(!profile.exists());

    let publication_root = root.path().join("publication");
    fs::create_dir(&publication_root).unwrap();
    let inputs = qualification_inputs_with_base_url(
        &publication_root,
        "macos",
        "aarch64",
        "https://github.com/example/release",
    );
    let profile = home.join("qualification-profiles/publication");
    assert!(matches!(
        install_macos_qualification(
            &home,
            &profile,
            &inputs.catalog.join("catalog.json"),
            &inputs.catalog.join("catalog.sig"),
            &inputs.artifacts,
            &inputs.public_key,
        ),
        Err(QualificationInstallError::PublishedArtifactLocation)
    ));
    assert!(!profile.exists());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn mutate_signature(inputs: &QualificationInputs) {
    let signature = inputs.catalog.join("catalog.sig");
    let mut bytes = fs::read(&signature).unwrap();
    bytes[0] ^= 1;
    fs::write(signature, bytes).unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn mutate_checksum(inputs: &QualificationInputs) {
    fs::write(
        inputs.artifacts.join("production-worker"),
        b"changed payload",
    )
    .unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn add_extra_payload(inputs: &QualificationInputs) {
    fs::write(inputs.artifacts.join("unexpected"), b"extra").unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn replace_payload_with_symlink(inputs: &QualificationInputs) {
    let payload = inputs.artifacts.join("production-worker");
    let outside = inputs.artifacts.parent().unwrap().join("outside-payload");
    fs::write(&outside, b"fixture worker").unwrap();
    fs::remove_file(&payload).unwrap();
    std::os::unix::fs::symlink(outside, payload).unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
struct QualificationInputs {
    catalog: std::path::PathBuf,
    artifacts: std::path::PathBuf,
    public_key: std::path::PathBuf,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn qualification_inputs(root: &Path, os: &str, architecture: &str) -> QualificationInputs {
    qualification_inputs_with_base_url(root, os, architecture, "https://qualification.invalid")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn qualification_inputs_with_base_url(
    root: &Path,
    os: &str,
    architecture: &str,
    base_url: &str,
) -> QualificationInputs {
    let artifacts = root.join("artifacts");
    let catalog = root.join("catalog");
    fs::create_dir_all(&artifacts).unwrap();
    let model_member = root.join("model.onnx");
    fs::write(&model_member, b"fixture model member").unwrap();
    let model_path = artifacts.join("production-model");
    write_model_pack(
        &model_path,
        &ModelPackSpec {
            kind: ModelPackKind::OnnxTransformerMeanPool,
            model_id: production_pipeline_identity()
                .model()
                .model_id()
                .as_str()
                .to_owned(),
            model_revision: production_pipeline_identity()
                .model()
                .revision()
                .as_str()
                .to_owned(),
            tokenizer: PRODUCTION_TOKENIZER_ID.to_owned(),
            dimensions: 384,
            max_input_tokens: 512,
            query_prefix: "query: ".to_owned(),
            passage_prefix: "passage: ".to_owned(),
            production: true,
            source: "fixture".to_owned(),
            files: vec![("model.onnx".to_owned(), model_member)],
        },
    )
    .unwrap();
    fs::write(artifacts.join("production-worker"), b"fixture worker").unwrap();
    fs::write(artifacts.join("production-runtime"), b"fixture runtime").unwrap();

    let target = TargetTriple::new(os, architecture).unwrap();
    let runtime_id = ArtifactId::new("production-runtime").unwrap();
    let worker_id = ArtifactId::new("production-worker").unwrap();
    let model_id = ArtifactId::new("production-model").unwrap();
    let runtime_component = ComponentId::new(PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID).unwrap();
    let runtime_requirement = RuntimeCompatibility::new(
        runtime_component.clone(),
        VersionReq::parse("=1.0.0").unwrap(),
    );
    let model_identity = ModelIdentity::new(
        ModelId::new(production_pipeline_identity().model().model_id().as_str()).unwrap(),
        ModelRevision::new(production_pipeline_identity().model().revision().as_str()).unwrap(),
    );
    let metadata = ModelMetadata::new(
        model_identity.clone(),
        LicenseInfo::new("Apache-2.0", "fixture notice").unwrap(),
        TokenizerId::new(PRODUCTION_TOKENIZER_ID).unwrap(),
        384,
        EmbeddingNormalization::UnitLength,
        runtime_requirement.clone(),
        ["en"],
        fs::metadata(&model_path).unwrap().len(),
        1024,
    )
    .unwrap();
    let artifact = |id: ArtifactId,
                    component: ComponentId,
                    kind: ArtifactKind,
                    file: &str,
                    target: Option<TargetTriple>,
                    protocol: Option<ProtocolRange>,
                    runtimes: Vec<RuntimeCompatibility>| {
        let bytes = fs::read(artifacts.join(file)).unwrap();
        CatalogArtifact::new(
            id,
            component,
            kind,
            Version::parse("1.0.0").unwrap(),
            ArtifactLocation::new(format!("{base_url}/{file}")).unwrap(),
            LicenseInfo::new("Apache-2.0", "fixture notice").unwrap(),
            Sha256Digest::calculate(&bytes),
            ComponentResources::new(bytes.len() as u64, bytes.len() as u64, 1024).unwrap(),
            ArtifactCompatibility::new(target, protocol, runtimes, 2),
        )
        .unwrap()
    };
    let worker = artifact(
        worker_id.clone(),
        ComponentId::new(PRODUCTION_WORKER_COMPONENT_ID).unwrap(),
        ArtifactKind::Worker,
        "production-worker",
        Some(target.clone()),
        Some(ProtocolRange::new(1, 1).unwrap()),
        vec![runtime_requirement.clone()],
    );
    let runtime = artifact(
        runtime_id.clone(),
        runtime_component,
        ArtifactKind::Runtime,
        "production-runtime",
        Some(target),
        None,
        Vec::new(),
    );
    let model = artifact(
        model_id.clone(),
        ComponentId::new(PRODUCTION_MODEL_COMPONENT_ID).unwrap(),
        ArtifactKind::Model(model_identity.clone()),
        "production-model",
        None,
        None,
        vec![runtime_requirement],
    );
    let manifest = CatalogManifest::new(
        ManifestRevision::new("qualification-fixture").unwrap(),
        vec![worker, runtime, model],
        vec![ModelManifest::new(model_id, metadata)],
        [(SemanticProfile::MultilingualQuality, model_identity)].into(),
    )
    .unwrap();
    let provenance = [
        worker_id,
        runtime_id,
        ArtifactId::new("production-model").unwrap(),
    ]
    .into_iter()
    .map(|id| {
        ProductionArtifactProvenance::new(
            id,
            ArtifactLocation::new("https://github.com/example/fixture").unwrap(),
            ManifestRevision::new("fixture-revision").unwrap(),
        )
    })
    .collect();
    let production =
        ProductionCatalogManifest::new(manifest, production_pipeline_identity(), provenance)
            .unwrap();
    let signing_key = SigningKey::from_bytes(&[0x42; 32]);
    let signed = sign_production_catalog(production, &signing_key).unwrap();
    write_signed_production_catalog(&signed, &catalog).unwrap();
    let public_key = root.join("catalog.pub");
    fs::write(&public_key, signing_key.verifying_key().as_bytes()).unwrap();
    let _ = PRODUCTION_ONNX_RUNTIME_COMPONENT_ID;
    QualificationInputs {
        catalog,
        artifacts,
        public_key,
    }
}
