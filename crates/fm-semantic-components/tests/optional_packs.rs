//! Signed optional-pack lifecycle and compatibility tests.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signer, SigningKey};
use fm_semantic_components::{
    AdvancedCapabilityKind, AdvancedEvaluationReport, AdvancedFixture, AdvancedFixtureResult,
    AdvancedPackDependency, AdvancedPackError, AdvancedPackKind, AdvancedPackManifest,
    AdvancedPackRegistry, AdvancedPackResources, DOCLING_PDF_PACK_ID, DOCLING_PDF_TARGETS,
    DOCLING_RS_REVISION, DOCLING_RS_VERSION, DoclingPackRelease, SignedAdvancedPackManifest,
    TrustedAdvancedPack,
};
use sha2::{Digest, Sha256};

fn report() -> AdvancedEvaluationReport {
    AdvancedEvaluationReport {
        baseline_fingerprint: "baseline-v1".into(),
        candidate_fingerprint: "candidate-v2".into(),
        baseline_ndcg: 0.50,
        candidate_ndcg: 0.54,
        p95_latency_ms: 80,
        peak_memory_bytes: 128 * 1024 * 1024,
        storage_bytes: 64 * 1024 * 1024,
        fixture_results: AdvancedFixture::all()
            .iter()
            .copied()
            .map(|fixture| {
                (
                    fixture,
                    AdvancedFixtureResult {
                        baseline: 1.0,
                        candidate: 1.1,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
        migration_impact: "rebuild candidate index".into(),
    }
}

#[test]
fn docling_release_manifest_is_pdf_only_and_target_specific() {
    let release = DoclingPackRelease {
        target: DOCLING_PDF_TARGETS[0].into(),
        artifact_url: "https://packages.example/docling-pdf-aarch64-apple-darwin.pack".into(),
        artifact_sha256: "01".repeat(32),
        dependencies: dependencies(),
        protocol_min: 1,
        protocol_max: 1,
        index_schema_version: 5,
        resources: AdvancedPackResources {
            download_bytes: 100,
            installed_bytes: 200,
            peak_ram_bytes: 300,
        },
        evaluation: report(),
    };
    let manifest = release.into_manifest();

    assert_eq!(manifest.id, DOCLING_PDF_PACK_ID);
    assert_eq!(manifest.version, DOCLING_RS_VERSION);
    assert_eq!(DOCLING_RS_REVISION.len(), 40);
    assert_eq!(
        manifest.targets,
        BTreeSet::from([DOCLING_PDF_TARGETS[0].into()])
    );
    assert_eq!(manifest.affected_formats, BTreeSet::from(["pdf".into()]));
}

fn dependencies() -> Vec<AdvancedPackDependency> {
    vec![AdvancedPackDependency {
        role: "converter".into(),
        version: DOCLING_RS_REVISION.into(),
        license: "MIT".into(),
        source_url: "https://crates.io/api/v1/crates/docling-pdf/1.36.0/download".into(),
        sha256: "02".repeat(32),
    }]
}

fn manifest(id: &str, kind: AdvancedPackKind, payload: &[u8]) -> AdvancedPackManifest {
    let capabilities = match kind {
        AdvancedPackKind::Converter => BTreeSet::from([
            AdvancedCapabilityKind::Ocr,
            AdvancedCapabilityKind::ComplexLayout,
            AdvancedCapabilityKind::Tables,
        ]),
        AdvancedPackKind::Acceleration => BTreeSet::from([AdvancedCapabilityKind::Gpu]),
        AdvancedPackKind::Reranker => BTreeSet::from([AdvancedCapabilityKind::CrossEncoder]),
    };
    AdvancedPackManifest {
        schema_version: 2,
        id: id.into(),
        version: "1.2.3".into(),
        kind,
        capabilities,
        affected_formats: if kind == AdvancedPackKind::Converter {
            BTreeSet::from(["pdf".into()])
        } else {
            BTreeSet::new()
        },
        dependencies: dependencies(),
        artifact_url: format!("https://packages.example/{id}.pack"),
        artifact_sha256: Sha256::digest(payload)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        targets: BTreeSet::from(["aarch64-apple-darwin".into()]),
        protocol_min: 1,
        protocol_max: 1,
        index_schema_version: 5,
        resources: AdvancedPackResources {
            download_bytes: payload.len() as u64,
            installed_bytes: 2 * payload.len() as u64,
            peak_ram_bytes: 128 * 1024 * 1024,
        },
        evaluation: report(),
    }
}

fn trusted(
    signing: &SigningKey,
    manifest: AdvancedPackManifest,
) -> Result<TrustedAdvancedPack, AdvancedPackError> {
    let signature = signing
        .sign(
            &manifest
                .canonical_bytes()
                .expect("fixture manifest is serializable"),
        )
        .to_bytes();
    TrustedAdvancedPack::verify(
        SignedAdvancedPackManifest::new(manifest, signature),
        &signing.verifying_key(),
        "aarch64-apple-darwin",
        1,
        5,
    )
}

#[test]
fn independently_signed_pack_rejects_tampering_and_incompatibility() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let payload = b"optional converter";
    let valid = manifest("ocr-pack", AdvancedPackKind::Converter, payload);
    trusted(&signing, valid.clone())
        .expect("trusted pack")
        .verify_payload(payload)
        .expect("payload");

    let signature = signing
        .sign(
            &valid
                .canonical_bytes()
                .expect("fixture manifest is serializable"),
        )
        .to_bytes();
    let mut tampered = valid.clone();
    tampered.version = "1.2.4".into();
    assert!(matches!(
        TrustedAdvancedPack::verify(
            SignedAdvancedPackManifest::new(tampered, signature),
            &signing.verifying_key(),
            "aarch64-apple-darwin",
            1,
            5,
        ),
        Err(AdvancedPackError::InvalidSignature)
    ));
    assert!(matches!(
        trusted(&signing, valid).and_then(|pack| pack.ensure_server_allowed(&BTreeSet::new())),
        Err(AdvancedPackError::ServerPolicyDenied)
    ));

    let mut portable = manifest("portable-ocr", AdvancedPackKind::Converter, payload);
    portable.targets = BTreeSet::from([
        "aarch64-apple-darwin".into(),
        "x86_64-pc-windows-msvc".into(),
        "x86_64-unknown-linux-gnu".into(),
    ]);
    let portable_signature = signing
        .sign(
            &portable
                .canonical_bytes()
                .expect("portable manifest is serializable"),
        )
        .to_bytes();
    for target in &portable.targets {
        TrustedAdvancedPack::verify(
            SignedAdvancedPackManifest::new(portable.clone(), portable_signature),
            &signing.verifying_key(),
            target,
            1,
            5,
        )
        .expect("declared packaging target");
    }
    assert!(matches!(
        TrustedAdvancedPack::verify(
            SignedAdvancedPackManifest::new(portable, portable_signature),
            &signing.verifying_key(),
            "aarch64-unknown-linux-gnu",
            1,
            5,
        ),
        Err(AdvancedPackError::Incompatible)
    ));
}

#[test]
fn lifecycle_keeps_one_rollback_and_removal_does_not_touch_other_kinds() {
    let signing = SigningKey::from_bytes(&[8; 32]);
    let mut registry = AdvancedPackRegistry::new();
    for (id, kind) in [
        ("ocr-v1", AdvancedPackKind::Converter),
        ("reranker-v1", AdvancedPackKind::Reranker),
    ] {
        let payload = id.as_bytes();
        registry
            .install(
                trusted(&signing, manifest(id, kind, payload)).expect("trusted fixture"),
                payload,
            )
            .expect("install fixture");
        registry.activate(id).expect("activate fixture");
    }
    let payload = b"ocr-v2";
    registry
        .install(
            trusted(
                &signing,
                manifest("ocr-v2", AdvancedPackKind::Converter, payload),
            )
            .expect("trusted replacement"),
            payload,
        )
        .expect("install replacement");
    let activation = registry.activate("ocr-v2").expect("activate replacement");
    assert_eq!(activation.affected_formats, BTreeSet::from(["pdf".into()]));
    assert_eq!(activation.rollback_id.as_deref(), Some("ocr-v1"));
    assert_eq!(activation.migration_impact, "rebuild candidate index");
    let no_op = registry.activate("ocr-v2").expect("no-op activation");
    assert_eq!(no_op.rollback_id.as_deref(), Some("ocr-v1"));
    assert_eq!(
        registry
            .status(AdvancedPackKind::Converter)
            .active_id
            .as_deref(),
        Some("ocr-v2")
    );
    registry
        .rollback(AdvancedPackKind::Converter)
        .expect("rollback converter");
    assert_eq!(
        registry
            .status(AdvancedPackKind::Converter)
            .active_id
            .as_deref(),
        Some("ocr-v1")
    );
    registry
        .remove(AdvancedPackKind::Converter)
        .expect("remove converter");
    assert!(
        registry
            .status(AdvancedPackKind::Converter)
            .active_id
            .is_none()
    );
    assert_eq!(
        registry
            .status(AdvancedPackKind::Reranker)
            .active_id
            .as_deref(),
        Some("reranker-v1")
    );
}

#[test]
fn legacy_schema_deserializes_only_to_report_unsupported_version() {
    let signing = SigningKey::from_bytes(&[10; 32]);
    let payload = b"legacy";
    let current = manifest("legacy", AdvancedPackKind::Converter, payload);
    let mut value = serde_json::to_value(current).expect("serialize current manifest");
    value["schemaVersion"] = serde_json::json!(1);
    value
        .as_object_mut()
        .expect("object")
        .remove("affectedFormats");
    value
        .as_object_mut()
        .expect("object")
        .remove("dependencies");
    let legacy: AdvancedPackManifest =
        serde_json::from_value(value).expect("deserialize legacy shape");
    let signature = signing
        .sign(&legacy.canonical_bytes().expect("canonical legacy bytes"))
        .to_bytes();

    assert!(matches!(
        TrustedAdvancedPack::verify(
            SignedAdvancedPackManifest::new(legacy, signature),
            &signing.verifying_key(),
            "aarch64-apple-darwin",
            1,
            5,
        ),
        Err(AdvancedPackError::UnsupportedSchema(1))
    ));
}

#[test]
fn evaluation_gate_requires_every_quality_and_cost_fixture() {
    let signing = SigningKey::from_bytes(&[9; 32]);
    let payload = b"reranker";
    let mut incomplete = manifest("reranker", AdvancedPackKind::Reranker, payload);
    incomplete
        .evaluation
        .fixture_results
        .remove(&AdvancedFixture::Ocr);
    assert!(matches!(
        trusted(&signing, incomplete),
        Err(AdvancedPackError::IncompleteEvaluation)
    ));

    let mut no_gain = manifest("reranker", AdvancedPackKind::Reranker, payload);
    no_gain.evaluation.candidate_ndcg = no_gain.evaluation.baseline_ndcg;
    assert!(matches!(
        trusted(&signing, no_gain),
        Err(AdvancedPackError::InsufficientQualityGain)
    ));

    let mut converter_no_gain = manifest("converter", AdvancedPackKind::Converter, payload);
    converter_no_gain.evaluation.candidate_ndcg = converter_no_gain.evaluation.baseline_ndcg;
    assert!(matches!(
        trusted(&signing, converter_no_gain),
        Err(AdvancedPackError::InsufficientQualityGain)
    ));
}
