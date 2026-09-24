//! Exercises a signed production payload set through the managed lifecycle.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use ed25519_dalek::VerifyingKey;
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactId, ArtifactKind, ArtifactRequest,
    ArtifactSource, ArtifactSourceError, CatalogArtifact, ComponentManager, ComponentQuiescer,
    DataCategory, FreeSpaceError, FreeSpaceProbe, InstallEnvironment, InstallError,
    ProductionCatalogManifest, QuiesceError, SemanticDataRoot, SemanticProfile, SemanticStateError,
    SemanticStateStore, TargetTriple, TrustedCatalog, UninstallIndexDecision,
    verify_production_payloads, verify_serialized_production_catalog,
};
use serde::Serialize;

const CHUNK_BYTES: usize = 1024 * 1024;
const RESERVE_BYTES: u64 = 64 * 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("semantic installed lifecycle qualification: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if let [command, configuration, app_data, marker] = arguments.as_slice()
        && command == Path::new("--hold-lock")
    {
        let manager =
            ComponentManager::new(SemanticStateStore::new(configuration), app_data.to_owned());
        manager.run_serialized_lifecycle(|| {
            fs::write(marker, b"locked")?;
            thread::sleep(Duration::from_millis(400));
            Ok::<(), SemanticStateError>(())
        })?;
        return Ok(());
    }
    let [
        catalog_path,
        signature_path,
        artifacts,
        public_key_path,
        working_root,
        report_path,
    ] = arguments.try_into().map_err(|_| {
        "usage: qualify_semantic_lifecycle <catalog.json> <catalog.sig> <artifacts> \
                 <public-key> <working-root> <report.json>"
    })?;
    if working_root.exists() {
        return Err("qualification working root must not already exist".into());
    }
    fs::create_dir_all(&working_root)?;

    let catalog_bytes = fs::read(&catalog_path)?;
    let signature_bytes = fs::read(&signature_path)?;
    let public_key: [u8; 32] = fs::read(&public_key_path)?
        .try_into()
        .map_err(|_| "production catalog public key must contain exactly 32 bytes")?;
    let public_key = VerifyingKey::from_bytes(&public_key)?;
    let manifest: ProductionCatalogManifest = serde_json::from_slice(&catalog_bytes)?;
    let catalog =
        verify_serialized_production_catalog(&catalog_bytes, &signature_bytes, &public_key)?;
    verify_production_payloads(&manifest, &artifacts)?;

    let target = qualification_target(&catalog)?;
    let selected = catalog.installation_artifacts(
        SemanticProfile::MultilingualQuality,
        &catalog
            .artifacts()
            .iter()
            .filter(|artifact| {
                !matches!(artifact.kind(), ArtifactKind::Model(_))
                    && artifact.compatibility().target() == Some(&target)
            })
            .map(|artifact| artifact.id().clone())
            .collect::<Vec<_>>(),
    )?;
    let configuration = working_root.join("configuration");
    let app_data = working_root.join("app-data");
    let semantic_root = SemanticDataRoot::from_app_data(&app_data);
    let environment = InstallEnvironment::new(
        target.clone(),
        manifest.pipeline().worker_protocol_version(),
        BTreeMap::new(),
    );
    let mut checks = Vec::new();

    let manager = ComponentManager::new(SemanticStateStore::new(&configuration), &app_data);
    if !manager.state()?.installed_components().is_empty() || semantic_root.path().exists() {
        return Err("absent lifecycle state was not inert".into());
    }
    checks.push(pass(
        "installed-absent",
        "No components, semantic data directory, worker, or artifact read existed before consent.",
    ));

    let low_disk_source = LocalArtifactSource::new(&artifacts);
    let low_disk = manager.install(
        offer(&catalog, &selected, &target, semantic_root.path())?.consent(),
        &catalog,
        &environment,
        &low_disk_source,
        &FixedFreeSpace(0),
        &VerifyActivation,
    );
    if !matches!(low_disk, Err(InstallError::InsufficientSpace { .. }))
        || low_disk_source.requests() != 0
    {
        return Err("low-disk admission did not fail before artifact access".into());
    }
    checks.push(pass(
        "low-disk",
        "Installation failed before the local artifact source was read.",
    ));

    let mut tampered_catalog = catalog_bytes.clone();
    let tamper_offset = tampered_catalog
        .iter()
        .position(|byte| *byte == b'{')
        .ok_or("catalog JSON was empty")?;
    tampered_catalog[tamper_offset] = b'[';
    if verify_serialized_production_catalog(&tampered_catalog, &signature_bytes, &public_key)
        .is_ok()
    {
        return Err("tampered catalog was accepted".into());
    }
    checks.push(pass(
        "corrupt-catalog",
        "A modified catalog failed detached-signature or manifest verification.",
    ));

    let first_artifact = selected
        .first()
        .ok_or("installation plan was empty")?
        .clone();
    let corrupt_root = working_root.join("corrupt-payload");
    let corrupt_manager = ComponentManager::new(
        SemanticStateStore::new(corrupt_root.join("configuration")),
        corrupt_root.join("app-data"),
    );
    let corrupt_source = LocalArtifactSource::corrupt_once(&artifacts, first_artifact.clone());
    let corrupt_install = corrupt_manager.install(
        offer(
            &catalog,
            &selected,
            &target,
            SemanticDataRoot::from_app_data(&corrupt_root.join("app-data")).path(),
        )?
        .consent(),
        &catalog,
        &environment,
        &corrupt_source,
        &FixedFreeSpace(u64::MAX),
        &VerifyActivation,
    );
    if !matches!(corrupt_install, Err(InstallError::ChecksumMismatch { .. })) {
        return Err("corrupt payload did not fail checksum verification".into());
    }
    checks.push(pass(
        "corrupt-payload",
        "A modified local artifact stream failed SHA-256 verification before activation.",
    ));
    fs::remove_dir_all(corrupt_root)?;

    let source = LocalArtifactSource::interrupt_once(&artifacts, first_artifact);
    let interrupted = manager.install(
        offer(&catalog, &selected, &target, semantic_root.path())?.consent(),
        &catalog,
        &environment,
        &source,
        &FixedFreeSpace(u64::MAX),
        &VerifyActivation,
    );
    if !matches!(interrupted, Err(InstallError::DownloadInterrupted { .. }))
        || !manager.state()?.installed_components().is_empty()
    {
        return Err("interrupted install exposed partial component state".into());
    }
    manager.install(
        offer(&catalog, &selected, &target, semantic_root.path())?.consent(),
        &catalog,
        &environment,
        &source,
        &FixedFreeSpace(u64::MAX),
        &VerifyActivation,
    )?;
    let resume_offsets = source.offsets_for(
        selected
            .first()
            .ok_or("installation plan was empty after resume")?,
    );
    if resume_offsets.iter().filter(|offset| **offset == 0).count() != 1
        || !resume_offsets.iter().any(|offset| *offset > 0)
    {
        return Err("retry did not resume the interrupted artifact at a nonzero offset".into());
    }
    checks.push(pass(
        "cancellation-resume",
        "An interrupted local artifact read published nothing and resumed by byte offset.",
    ));
    checks.push(pass(
        "first-install-offline",
        "The exact payload set installed through a filesystem-only ArtifactSource.",
    ));

    let restarted = ComponentManager::new(SemanticStateStore::new(&configuration), &app_data);
    let installed = restarted.state()?;
    if installed.installed_components().len() != selected.len() {
        return Err("restart did not recover every installed component".into());
    }
    for artifact in catalog
        .artifacts()
        .iter()
        .filter(|artifact| selected.contains(artifact.id()))
    {
        if restarted.verified_installed_payload(artifact)?.is_none() {
            return Err(format!("restart could not verify {}", artifact.id().as_str()).into());
        }
    }
    checks.push(pass(
        "app-restart",
        "A new manager recovered durable state and revalidated every active payload.",
    ));

    let worker = catalog
        .artifacts()
        .iter()
        .find(|artifact| {
            selected.contains(artifact.id()) && matches!(artifact.kind(), ArtifactKind::Worker)
        })
        .ok_or("selected worker was absent")?;
    let installed_worker = restarted
        .verified_installed_payload(worker)?
        .ok_or("installed worker was absent")?;
    let mut worker_bytes = fs::read(&installed_worker)?;
    worker_bytes[0] ^= 0x01;
    fs::write(&installed_worker, &worker_bytes)?;
    if !matches!(
        restarted.verified_installed_payload(worker),
        Err(InstallError::InstalledArtifactInvalid { .. })
    ) {
        return Err("tampered installed worker was accepted".into());
    }
    fs::copy(artifacts.join(worker.id().as_str()), &installed_worker)?;
    restarted
        .verified_installed_payload(worker)?
        .ok_or("restored installed worker did not verify")?;
    checks.push(pass(
        "corrupt-installed-component",
        "Worker launch resolution rejected changed bytes and accepted only restored catalog bytes.",
    ));

    qualify_concurrent_lifecycle(&restarted, &configuration, &app_data, &working_root)?;
    checks.push(pass(
        "concurrent-instance",
        "Independent manager handles serialized lifecycle mutation through the durable lock.",
    ));

    let derived_index = semantic_root
        .category_path(DataCategory::Zvec)
        .join("qualification-corrupt-index.bin");
    fs::write(&derived_index, b"intentionally malformed derived index")?;
    let retained = restarted.uninstall(UninstallIndexDecision::Retain, &ReadyQuiescer)?;
    if !derived_index.is_file() || retained.removed_component_count() == 0 {
        return Err("uninstall retention removed derived index data".into());
    }
    checks.push(pass(
        "uninstall-retention",
        "Explicit retention removed components while preserving the derived index.",
    ));

    restarted.install(
        offer(&catalog, &selected, &target, semantic_root.path())?.consent(),
        &catalog,
        &environment,
        &LocalArtifactSource::new(&artifacts),
        &FixedFreeSpace(u64::MAX),
        &VerifyActivation,
    )?;
    let deleted = restarted.uninstall(UninstallIndexDecision::Delete, &ReadyQuiescer)?;
    if derived_index.exists()
        || deleted.index_decision() != UninstallIndexDecision::Delete
        || !restarted.state()?.installed_components().is_empty()
    {
        return Err("explicit deletion retained component or derived-index data".into());
    }
    checks.push(pass(
        "explicit-deletion",
        "Explicit deletion removed components and the malformed derived index.",
    ));

    restarted.install(
        offer(&catalog, &selected, &target, semantic_root.path())?.consent(),
        &catalog,
        &environment,
        &LocalArtifactSource::new(&artifacts),
        &FixedFreeSpace(u64::MAX),
        &VerifyActivation,
    )?;
    if restarted.state()?.installed_components().len() != selected.len() {
        return Err("clean reinstall after deletion was incomplete".into());
    }
    checks.push(pass(
        "rebuild-recovery",
        "A clean reinstall recovered the component generation after explicit derived-data deletion.",
    ));
    restarted.uninstall(UninstallIndexDecision::Delete, &ReadyQuiescer)?;

    checks.push(QualificationCheck {
        id: "preceding-candidate-upgrade-rollback",
        status: "blocked",
        detail: "No exact signed preceding production candidate was supplied; synthetic upgrade evidence is forbidden.",
        rollback: "Retain the immediately preceding signed catalog and payload set, then rerun this harness with both candidates.",
    });
    checks.extend([
        manual(
            "native-screen-reader",
            "VoiceOver, Narrator, and Orca evidence requires native assistive-technology operators.",
        ),
        manual(
            "keyboard-consent-progress-errors-citations-deletion",
            "Packaged UI behavior requires the operator checklist; automation does not claim human UX evidence.",
        ),
    ]);

    let report = LifecycleReport {
        schema_version: 1,
        target: format!("{}-{}", target.operating_system(), target.architecture()),
        catalog_revision: catalog.revision().as_str(),
        catalog_sha256: sha256_hex(&catalog_bytes),
        signature_sha256: sha256_hex(&signature_bytes),
        artifacts: catalog
            .artifacts()
            .iter()
            .map(|artifact| ArtifactEvidence {
                id: artifact.id().as_str(),
                component: artifact.component_id().as_str(),
                bytes: artifact.resources().download_bytes(),
                sha256: digest_hex(artifact),
            })
            .collect(),
        checks,
        network_artifact_reads: 0,
        rollback: "Disable SEMANTIC_RELEASE_QUALIFIED; restore the preceding immutable signed catalog; retain indexes unless deletion was explicitly requested.",
    };
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(report_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn qualification_target(catalog: &TrustedCatalog) -> Result<TargetTriple, Box<dyn Error>> {
    let targets = catalog
        .artifacts()
        .iter()
        .filter(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
        .filter_map(|artifact| artifact.compatibility().target())
        .collect::<Vec<_>>();
    if targets.len() != 1 {
        return Err("catalog must contain exactly one target-specific worker".into());
    }
    Ok(targets[0].clone())
}

fn offer(
    catalog: &TrustedCatalog,
    selected: &[ArtifactId],
    target: &TargetTriple,
    root: &Path,
) -> Result<fm_semantic_components::InstallationOffer, Box<dyn Error>> {
    Ok(catalog.installation_offer(
        SemanticProfile::MultilingualQuality,
        selected,
        target,
        1,
        root,
        RESERVE_BYTES,
    )?)
}

fn qualify_concurrent_lifecycle(
    manager: &ComponentManager,
    configuration: &Path,
    app_data: &Path,
    working_root: &Path,
) -> Result<(), Box<dyn Error>> {
    let marker = working_root.join("child-holds-lifecycle-lock");
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .arg("--hold-lock")
        .arg(configuration)
        .arg(app_data)
        .arg(&marker)
        .spawn()?;
    for _ in 0..500 {
        if marker.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !marker.is_file() {
        let _ = child.kill();
        return Err("separate process did not acquire the lifecycle lock".into());
    }
    let started = Instant::now();
    manager.run_serialized_lifecycle(|| Ok::<(), SemanticStateError>(()))?;
    let status = child.wait()?;
    if !status.success() {
        return Err("separate lifecycle process failed".into());
    }
    if started.elapsed() < Duration::from_millis(250) {
        return Err("concurrent lifecycle mutation bypassed the durable lock".into());
    }
    Ok(())
}

struct LocalArtifactSource {
    root: PathBuf,
    requests: AtomicUsize,
    offsets: Mutex<Vec<(ArtifactId, u64)>>,
    interrupt_artifact: Option<ArtifactId>,
    interrupt_once: AtomicBool,
    corrupt_artifact: Option<ArtifactId>,
}

impl LocalArtifactSource {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            requests: AtomicUsize::new(0),
            offsets: Mutex::new(Vec::new()),
            interrupt_artifact: None,
            interrupt_once: AtomicBool::new(false),
            corrupt_artifact: None,
        }
    }

    fn interrupt_once(root: &Path, artifact: ArtifactId) -> Self {
        Self {
            root: root.to_owned(),
            requests: AtomicUsize::new(0),
            offsets: Mutex::new(Vec::new()),
            interrupt_artifact: Some(artifact),
            interrupt_once: AtomicBool::new(true),
            corrupt_artifact: None,
        }
    }

    fn corrupt_once(root: &Path, artifact: ArtifactId) -> Self {
        Self {
            root: root.to_owned(),
            requests: AtomicUsize::new(0),
            offsets: Mutex::new(Vec::new()),
            interrupt_artifact: None,
            interrupt_once: AtomicBool::new(false),
            corrupt_artifact: Some(artifact),
        }
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }

    fn offsets_for(&self, artifact: &ArtifactId) -> Vec<u64> {
        self.offsets
            .lock()
            .expect("qualification offset log")
            .iter()
            .filter(|(candidate, _)| candidate == artifact)
            .map(|(_, offset)| *offset)
            .collect()
    }
}

impl ArtifactSource for LocalArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.offsets
            .lock()
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?
            .push((request.artifact_id().clone(), request.offset()));
        if self.interrupt_artifact.as_ref() == Some(request.artifact_id())
            && request.offset() > 0
            && self.interrupt_once.swap(false, Ordering::AcqRel)
        {
            return Err(ArtifactSourceError::Unavailable(
                "injected qualification interruption".to_owned(),
            ));
        }
        let path = self.root.join(request.artifact_id().as_str());
        let mut file = fs::File::open(&path)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        let length = file
            .metadata()
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?
            .len();
        if request.offset() > length {
            return Err(ArtifactSourceError::InvalidOffset {
                offset: request.offset(),
            });
        }
        file.seek(SeekFrom::Start(request.offset()))
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        let remaining = length.saturating_sub(request.offset());
        let wanted = usize::try_from(remaining.min(CHUNK_BYTES as u64))
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        let mut bytes = vec![0; wanted];
        file.read_exact(&mut bytes)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        if request.offset() == 0
            && self.corrupt_artifact.as_ref() == Some(request.artifact_id())
            && let Some(first) = bytes.first_mut()
        {
            *first ^= 0x01;
        }
        Ok(ArtifactChunk::new(
            bytes,
            request.offset() + u64::try_from(wanted).unwrap_or(u64::MAX) == length,
        ))
    }
}

struct FixedFreeSpace(u64);

impl FreeSpaceProbe for FixedFreeSpace {
    fn available_bytes(&self, _path: &Path) -> Result<u64, FreeSpaceError> {
        Ok(self.0)
    }
}

struct VerifyActivation;

impl ActivationProbe for VerifyActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        installed_path: &Path,
    ) -> Result<(), ActivationError> {
        let metadata = fs::metadata(installed_path)
            .map_err(|error| ActivationError::new(error.to_string()))?;
        if !metadata.is_file() || metadata.len() != artifact.resources().download_bytes() {
            return Err(ActivationError::new(
                "installed artifact does not match signed size",
            ));
        }
        Ok(())
    }
}

struct ReadyQuiescer;

impl ComponentQuiescer for ReadyQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LifecycleReport<'a> {
    schema_version: u32,
    target: String,
    catalog_revision: &'a str,
    catalog_sha256: String,
    signature_sha256: String,
    artifacts: Vec<ArtifactEvidence<'a>>,
    checks: Vec<QualificationCheck<'a>>,
    network_artifact_reads: u64,
    rollback: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactEvidence<'a> {
    id: &'a str,
    component: &'a str,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QualificationCheck<'a> {
    id: &'a str,
    status: &'a str,
    detail: &'a str,
    rollback: &'a str,
}

fn pass(id: &'static str, detail: &'static str) -> QualificationCheck<'static> {
    QualificationCheck {
        id,
        status: "pass",
        detail,
        rollback: "Restore the preceding immutable signed catalog and retain user data by default.",
    }
}

fn manual(id: &'static str, detail: &'static str) -> QualificationCheck<'static> {
    QualificationCheck {
        id,
        status: "manual-required",
        detail,
        rollback: "Do not enable SEMANTIC_RELEASE_QUALIFIED until an operator records the required evidence.",
    }
}

fn digest_hex(artifact: &CatalogArtifact) -> String {
    artifact
        .checksum()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
