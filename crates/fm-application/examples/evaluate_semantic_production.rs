//! Runs the task-0188 corpus through an exact packaged semantic worker.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fm_application::rag::{RagRetrievalCapability, SemanticRagRetrievalCapability};
use fm_application::semantic::{
    DocumentId, DocumentIngestion, IpcSemanticCapability, LibraryId, SemanticCapability,
    SemanticIngestionState, SemanticJobId, SemanticOperationId, SemanticQuery, SemanticScope,
    SemanticService, TenantId,
};
use fm_application::semantic_production_evaluation::{
    CitationObservation, CorpusScope, CorpusSource, ProductionArtifactIdentity,
    ProductionCandidateIdentity, ProductionCaseObservation, ProductionEvaluationCorpus,
    ProductionEvaluationReport, ProductionTargetMeasurement, RankedChunkEvidence,
    RetrievalPolicyIdentity,
};
use fm_semantic_components::{
    PRODUCTION_MODEL_COMPONENT_ID, PRODUCTION_WORKER_COMPONENT_ID,
    PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID, ProductionCatalogManifest, verify_production_payloads,
};
use fm_semantic_conversion::{ChunkProvenance, Provenance};
use fm_semantic_worker::rag_retrieval::{
    RagContext, RagRetrievalPolicy, RagRetrievalRequest, RagSourceRestriction,
};
use fm_semantic_worker::semantic_search::SemanticEvidence;
use fm_semantic_worker::semantic_storage::QueryFilters;
use fm_semantic_worker::{ManagedWorkerLaunch, ManagedWorkerResolver};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use zip::write::SimpleFileOptions;

const AUTHORIZED_TENANT: &str = "evaluation-tenant";
const EXCLUDED_TENANT: &str = "evaluation-excluded-tenant";
const LIBRARY: &str = "evaluation-library";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QualificationRecord {
    artifact_id: String,
    build: QualificationBuild,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QualificationBuild {
    revision: String,
    working_tree: String,
}

fn required_argument(
    arguments: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("expected {name}").into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args().skip(1);
    let bundle = required_argument(&mut arguments, "production bundle directory")?;
    let output = required_argument(&mut arguments, "private evaluation report path")?;
    if arguments.next().is_some() {
        return Err("unexpected extra argument".into());
    }

    let corpus = ProductionEvaluationCorpus::parse(include_str!(
        "../tests/fixtures/semantic-evaluation-v1.json"
    ))?;
    let manifest_bytes = fs::read(bundle.join("catalog-input.json"))?;
    let manifest: ProductionCatalogManifest = serde_json::from_slice(&manifest_bytes)?;
    manifest.validate()?;
    verify_production_payloads(&manifest, &bundle.join("artifacts"))?;

    let worker_artifact = artifact(&manifest, PRODUCTION_WORKER_COMPONENT_ID)?;
    let model_artifact = artifact(&manifest, PRODUCTION_MODEL_COMPONENT_ID)?;
    let zvec_artifact = artifact(&manifest, PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID)?;
    let worker = bundle.join("artifacts").join(worker_artifact.id().as_str());
    let model = bundle.join("artifacts").join(model_artifact.id().as_str());
    let native_directory = PathBuf::from(
        env::var_os("PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY")
            .ok_or("PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY is required")?,
    );
    if !worker.is_file() || !model.exists() || !native_directory.is_dir() {
        return Err("packaged worker, model, or isolated native runtime is unavailable".into());
    }

    let target = worker_artifact
        .compatibility()
        .target()
        .ok_or("production worker has no target")?;
    let target_label = format!("{}-{}", target.operating_system(), target.architecture());
    let worker_provenance = manifest
        .provenance()
        .iter()
        .find(|record| record.artifact_id() == worker_artifact.id())
        .ok_or("worker provenance is missing")?;
    let procyon_revision = worker_provenance.source_revision().as_str().to_owned();

    let zvec_qualification_path = bundle.join("zvec-runtime-qualification.json");
    let zvec_qualification = qualification(&zvec_qualification_path, zvec_artifact.id().as_str())?;
    let onnx_qualification_path = bundle.join("onnx-runtime-qualification.json");
    let onnx_qualification = if onnx_qualification_path.is_file() {
        Some(qualification(
            &onnx_qualification_path,
            "procyon.semantic.onnx-runtime",
        )?)
    } else {
        None
    };
    let strict_production =
        env::var("PROCYON_SEMANTIC_PRODUCTION_TRUST_VERIFIED").as_deref() == Ok("1");
    for record in std::iter::once(&zvec_qualification).chain(onnx_qualification.as_ref()) {
        if record.build.revision != procyon_revision
            || (strict_production && record.build.working_tree != "clean")
        {
            return Err(
                "runtime qualification is not bound to the required production revision".into(),
            );
        }
    }

    let runtime = TempDir::new()?;
    let data = TempDir::new()?;
    let launch = ManagedWorkerLaunch::new(
        worker,
        data.path().join("semantic-data"),
        native_directory,
        model,
    );
    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(launch.clone()));
    let capability = Arc::new(IpcSemanticCapability::desktop_managed(
        runtime.path(),
        resolver,
    ));
    let rag = SemanticRagRetrievalCapability::new(SemanticService::new(capability.clone()));

    for (index, document) in corpus.documents.iter().enumerate() {
        let tenant = match document.scope {
            CorpusScope::Authorized => AUTHORIZED_TENANT,
            CorpusScope::Excluded => EXCLUDED_TENANT,
        };
        let metadata = BTreeMap::from([
            (
                "occurrence_id".into(),
                format!("occurrence-{}", document.id),
            ),
            ("source_id".into(), document.source_id.clone()),
            (
                "root_id".into(),
                match document.scope {
                    CorpusScope::Authorized => "evaluation-root".into(),
                    CorpusScope::Excluded => "excluded-root".into(),
                },
            ),
            (
                "workspace_id".into(),
                match document.scope {
                    CorpusScope::Authorized => "evaluation-workspace".into(),
                    CorpusScope::Excluded => "excluded-workspace".into(),
                },
            ),
            ("modified_at_ms".into(), "1".into()),
        ]);
        if let Some(previous_source) = &document.previous_source {
            let job_id = capability
                .ingest(DocumentIngestion {
                    scope: semantic_scope(tenant),
                    operation_id: SemanticOperationId::new(format!(
                        "evaluation-ingest-{index}-previous"
                    )),
                    document_id: DocumentId::new(document.id.clone()),
                    metadata: metadata.clone(),
                    media_type: document.media_type.clone(),
                    content: source_bytes(previous_source)?,
                })
                .await?;
            wait_for_ingestion(capability.as_ref(), tenant, job_id).await?;
        }
        let content = source_bytes(&document.source)?;
        let job_id = capability
            .ingest(DocumentIngestion {
                scope: semantic_scope(tenant),
                operation_id: SemanticOperationId::new(format!("evaluation-ingest-{index}")),
                document_id: DocumentId::new(document.id.clone()),
                metadata,
                media_type: document.media_type.clone(),
                content,
            })
            .await?;
        wait_for_ingestion(capability.as_ref(), tenant, job_id).await?;
    }

    let policy = RagRetrievalPolicy::default_ask();
    let mut observations = Vec::with_capacity(corpus.cases.len());
    for case in &corpus.cases {
        let results = capability
            .query(SemanticQuery {
                scope: semantic_scope(AUTHORIZED_TENANT),
                request_id: SemanticOperationId::new(format!("evaluation-query-{}", case.id)),
                text: case.query.clone(),
                concept: None,
                maximum_results: 128,
            })
            .await?;
        let case_variant = case.query.to_uppercase();
        if case_variant != case.query {
            let variant_results = capability
                .query(SemanticQuery {
                    scope: semantic_scope(AUTHORIZED_TENANT),
                    request_id: SemanticOperationId::new(format!(
                        "evaluation-query-case-variant-{}",
                        case.id
                    )),
                    text: case_variant,
                    concept: None,
                    maximum_results: 128,
                })
                .await?;
            if results != variant_results {
                return Err(format!(
                    "semantic ranking changed with query casing for `{}`",
                    case.id
                )
                .into());
            }
        }
        let request = RagRetrievalRequest {
            question: case.query.clone(),
            filters: QueryFilters {
                tenant_id: AUTHORIZED_TENANT.into(),
                library_id: Some(LIBRARY.into()),
                ..QueryFilters::default()
            },
            additional_tenant_ids: BTreeSet::new(),
            source_restriction: RagSourceRestriction::default(),
            current_hashes: HashMap::new(),
            policy,
        };
        let cancellation = CancellationToken::new();
        let candidates = rag
            .retrieve_candidates(request.clone(), &cancellation)
            .await?;
        let context = rag.pack_candidates(request, candidates).await?;
        let mut raw = Vec::new();
        for result in results {
            let document_id = result.document_id.as_str().to_owned();
            let document = corpus
                .document(&document_id)
                .ok_or_else(|| format!("worker returned unknown document `{document_id}`"))?;
            if document.scope != CorpusScope::Authorized {
                return Err(format!("worker returned excluded document `{document_id}`").into());
            }
            let evidence_json = result
                .metadata
                .get("semantic.evidence")
                .ok_or("worker result omitted semantic evidence")?;
            let evidence: Vec<SemanticEvidence> = serde_json::from_str(evidence_json)?;
            raw.push((document_id, evidence));
        }
        let scenario_exercised = match corpus.scenario_requirements[&case.id] {
            fm_application::semantic_production_evaluation::ProductionScenario::Standard => true,
            fm_application::semantic_production_evaluation::ProductionScenario::IncrementalEdit => {
                corpus
                    .documents
                    .iter()
                    .any(|document| {
                        case.relevant_file_ids.contains(&document.id)
                            && document.previous_source.is_some()
                    })
            }
            fm_application::semantic_production_evaluation::ProductionScenario::UnavailableSource
            | fm_application::semantic_production_evaluation::ProductionScenario::GeneratedSummary
            | fm_application::semantic_production_evaluation::ProductionScenario::ConceptLabel => {
                false
            }
        };
        observations.push(observation(
            &corpus,
            case,
            &raw,
            &context,
            scenario_exercised,
            policy,
        )?);
    }

    capability.shutdown(Duration::from_secs(10)).await?;

    let identity = ProductionCandidateIdentity {
        procyon_revision,
        target: target_label,
        catalog_revision: manifest.catalog().revision().as_str().into(),
        catalog_input_sha256: sha256(&manifest_bytes),
        pipeline: manifest.pipeline().clone(),
        retrieval_policy: RetrievalPolicyIdentity::production(),
        artifacts: artifact_identities(&manifest),
        zvec_qualification_sha256: sha256(&fs::read(&zvec_qualification_path)?),
        onnx_qualification_sha256: onnx_qualification
            .map(|_| fs::read(&onnx_qualification_path).map(|bytes| sha256(&bytes)))
            .transpose()?,
    };
    let measurement =
        ProductionTargetMeasurement::new(&corpus, identity, strict_production, observations)?;
    let report = ProductionEvaluationReport::from_measurements(
        &corpus,
        "Exact content-addressed production worker, packaged native runtime, pinned multilingual model, production converter/chunker, native Zvec index, and Ask retrieval policy; runtime execution was offline.",
        vec![
            "No answer provider or credential was supplied. Offline citation eligibility is measured, while generated-answer citation correctness remains unmeasured and blocks release.".into(),
            "Installed application lifecycle, accessibility, privacy, failure-mode, and release-owner criteria remain outside this automated target measurement.".into(),
        ],
        vec![measurement],
        ProductionEvaluationReport::pending_manual_criteria(),
    );
    report.validate(&corpus)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, report.to_json()?)?;
    println!("{}", output.display());
    Ok(())
}

fn artifact<'a>(
    manifest: &'a ProductionCatalogManifest,
    component_id: &str,
) -> Result<&'a fm_semantic_components::CatalogArtifact, Box<dyn std::error::Error>> {
    manifest
        .catalog()
        .artifacts()
        .iter()
        .find(|artifact| artifact.component_id().as_str() == component_id)
        .ok_or_else(|| format!("production catalog omitted `{component_id}`").into())
}

fn artifact_identities(manifest: &ProductionCatalogManifest) -> Vec<ProductionArtifactIdentity> {
    let mut identities = manifest
        .catalog()
        .artifacts()
        .iter()
        .map(|artifact| ProductionArtifactIdentity {
            component_id: artifact.component_id().as_str().into(),
            artifact_id: artifact.id().as_str().into(),
            version: artifact.version().to_string(),
            sha256: format!("sha256:{}", hexadecimal(artifact.checksum().as_bytes())),
            byte_length: artifact.resources().download_bytes(),
        })
        .collect::<Vec<_>>();
    identities.sort_by(|left, right| left.component_id.cmp(&right.component_id));
    identities
}

fn qualification(
    path: &Path,
    expected_artifact_or_component: &str,
) -> Result<QualificationRecord, Box<dyn std::error::Error>> {
    let record: QualificationRecord = serde_json::from_slice(&fs::read(path)?)?;
    if record.artifact_id != expected_artifact_or_component
        && !record
            .artifact_id
            .starts_with(expected_artifact_or_component)
    {
        return Err(format!(
            "qualification {} does not match `{expected_artifact_or_component}`",
            path.display()
        )
        .into());
    }
    Ok(record)
}

async fn wait_for_ingestion(
    capability: &dyn SemanticCapability,
    tenant: &str,
    job_id: SemanticJobId,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let status = capability
            .ingestion_job(semantic_scope(tenant), job_id.clone())
            .await?;
        match status.state {
            SemanticIngestionState::Completed => return Ok(()),
            SemanticIngestionState::Failed
            | SemanticIngestionState::Cancelled
            | SemanticIngestionState::Skipped => {
                return Err(format!(
                    "ingestion `{}` ended as {:?}: {:?}",
                    job_id.as_str(),
                    status.state,
                    status.detail
                )
                .into());
            }
            SemanticIngestionState::Pending | SemanticIngestionState::Running => {}
        }
        if Instant::now() >= deadline {
            return Err(format!("ingestion `{}` timed out", job_id.as_str()).into());
        }
        sleep(Duration::from_millis(50)).await;
    }
}

fn semantic_scope(tenant: &str) -> SemanticScope {
    SemanticScope::new(TenantId::new(tenant), LibraryId::new(LIBRARY))
}

fn observation(
    corpus: &ProductionEvaluationCorpus,
    case: &fm_application::semantic_evaluation::EvaluationCase,
    results: &[(String, Vec<SemanticEvidence>)],
    context: &RagContext,
    production_scenario_exercised: bool,
    policy: RagRetrievalPolicy,
) -> Result<ProductionCaseObservation, Box<dyn std::error::Error>> {
    let mut candidate_chunks = results
        .iter()
        .flat_map(|(file_id, evidence)| {
            evidence
                .iter()
                .map(|item| ranked_evidence(corpus, file_id, item))
        })
        .collect::<Result<Vec<_>, _>>()?;
    candidate_chunks.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    let eligible_scores = candidate_chunks
        .iter()
        .filter(|evidence| !evidence.generated)
        .map(|evidence| evidence.score as f32)
        .collect::<Vec<_>>();
    let strongest = eligible_scores.iter().copied().max_by(f32::total_cmp);
    let effective = policy.effective_minimum_score(eligible_scores);
    let mut ranked_file_ids = Vec::new();
    let mut ranked_chunks = Vec::new();
    for chunk in &context.chunks {
        if chunk.adjacent {
            continue;
        }
        let evidence = &chunk.evidence;
        let labelled = corpus
            .chunk_at(&evidence.document_id, evidence.source_position)
            .ok_or_else(|| {
                format!(
                    "packed production chunk `{}` position {} has no corpus label",
                    evidence.document_id, evidence.source_position
                )
            })?;
        if !ranked_file_ids.contains(&evidence.document_id) {
            ranked_file_ids.push(evidence.document_id.clone());
        }
        ranked_chunks.push(RankedChunkEvidence {
            chunk_id: labelled.id.clone(),
            file_id: evidence.document_id.clone(),
            score: f64::from(chunk.score),
            token_count: evidence.token_count,
            source_id: evidence.source_id.clone(),
            provenance_kind: provenance_kind_from_json(&evidence.provenance)?,
            unavailable: !evidence.available,
            stale: chunk.stale,
            generated: evidence.generated,
        });
    }
    let offline_citations = if case.expected_no_answer {
        Vec::new()
    } else {
        ranked_chunks
            .iter()
            .take(case.relevant_chunk_ids.len())
            .enumerate()
            .map(|(index, chunk)| CitationObservation {
                label: format!("C{}", index + 1),
                file_id: chunk.file_id.clone(),
                chunk_ids: vec![chunk.chunk_id.clone()],
            })
            .collect()
    };
    Ok(ProductionCaseObservation {
        case_id: case.id.clone(),
        ranked_file_ids,
        ranked_chunks,
        candidate_chunks,
        strongest_score: strongest.map(f64::from),
        effective_minimum_score: f64::from(effective),
        offline_citations,
        grounded_answer_citations: None,
        production_scenario_exercised,
    })
}

fn ranked_evidence(
    corpus: &ProductionEvaluationCorpus,
    file_id: &str,
    item: &SemanticEvidence,
) -> Result<RankedChunkEvidence, Box<dyn std::error::Error>> {
    let document = corpus
        .document(file_id)
        .ok_or_else(|| format!("unknown corpus document `{file_id}`"))?;
    let chunk = corpus
        .chunk_at(file_id, item.source_position)
        .ok_or_else(|| {
            format!(
                "production chunk `{file_id}` position {} has no corpus label",
                item.source_position
            )
        })?;
    let provenance_kind = provenance_kind(&item.provenance);
    if chunk.provenance_kind != provenance_kind || item.source_id != document.source_id {
        return Err(format!(
            "production provenance drift for `{file_id}` position {}",
            item.source_position
        )
        .into());
    }
    Ok(RankedChunkEvidence {
        chunk_id: chunk.id.clone(),
        file_id: file_id.into(),
        score: f64::from(item.score),
        token_count: item.excerpt.split_whitespace().count().max(1),
        source_id: item.source_id.clone(),
        provenance_kind: provenance_kind.into(),
        unavailable: item.unavailable,
        stale: item.stale,
        generated: item.generated,
    })
}

fn provenance_kind_from_json(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let provenance: ChunkProvenance = serde_json::from_str(value)?;
    Ok(provenance_kind(&provenance).into())
}

fn provenance_kind(provenance: &ChunkProvenance) -> &'static str {
    let provenance = match provenance {
        ChunkProvenance::Exact(value) => value,
        ChunkProvenance::Span { first, .. } => first,
    };
    match provenance {
        Provenance::TextLines { .. } => "textLines",
        Provenance::CodeLines { .. } => "codeLines",
        Provenance::PdfBlock { .. } => "pdfBlock",
        Provenance::Slide { .. } => "slide",
        Provenance::SpreadsheetRange { .. } => "spreadsheetRange",
        Provenance::DocxBlock { .. } => "docxBlock",
        Provenance::EpubText { .. } => "epubText",
    }
}

fn source_bytes(source: &CorpusSource) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    match source {
        CorpusSource::Text { content } => Ok(content.as_bytes().to_vec()),
        CorpusSource::Presentation { slides } => presentation(slides),
    }
}

fn presentation(slides: &[String]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("ppt/presentation.xml", options)?;
        archive.write_all(b"<p:presentation xmlns:p=\"x\"/>")?;
        for (index, text) in slides.iter().enumerate() {
            archive.start_file(format!("ppt/slides/slide{}.xml", index + 1), options)?;
            write!(
                archive,
                "<p:sld xmlns:p=\"x\" xmlns:a=\"y\"><p:sp><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:sp></p:sld>",
                escape_xml(text)
            )?;
        }
        archive.finish()?;
    }
    Ok(buffer.into_inner())
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hexadecimal(&Sha256::digest(bytes)))
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}
