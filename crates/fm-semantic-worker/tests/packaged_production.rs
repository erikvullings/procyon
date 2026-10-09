//! Cross-platform qualification of the exact packaged worker/runtime/model boundary.

#![cfg(feature = "semantic-runtime")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

#[cfg(feature = "gemma-native")]
use fm_semantic_worker::gemma_native::{GemmaMedia, GemmaNativeFiles};
use fm_semantic_worker::{
    IngestionScope, IngestionState, ManagedWorkerLaunch, ManagedWorkerResolver, WorkerConnector,
    WorkerHealth,
};

fn required_path(variable: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} is required")))
}

fn privacy_canaries() -> BTreeMap<String, String> {
    let path = std::env::var_os("PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE")
        .expect("PROCYON_SEMANTIC_PRIVACY_CANARIES_FILE is required");
    let value = std::fs::read_to_string(path).expect("read private privacy canary file");
    let canaries: BTreeMap<String, String> =
        serde_json::from_str(&value).expect("privacy canaries must be a string map");
    for required in [
        "query",
        "excerpt",
        "filename-path",
        "prompt",
        "response",
        "credential",
        "token",
        "authorization-header",
        "model-payload",
    ] {
        assert!(
            canaries.get(required).is_some_and(|value| value.len() >= 8),
            "missing required privacy canary category {required}"
        );
    }
    canaries
}

async fn wait_for_ingestion(
    client: &fm_semantic_worker::WorkerClient,
    job_id: &str,
    timeout: Duration,
) -> IngestionState {
    let started = Instant::now();
    while started.elapsed() < timeout {
        let state = client
            .ingestion_job("qualification-tenant", "qualification-library", job_id)
            .await
            .expect("read packaged ingestion")
            .state;
        if matches!(
            state,
            IngestionState::Completed
                | IngestionState::Failed
                | IngestionState::Cancelled
                | IngestionState::Skipped
        ) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("packaged ingestion did not finish within {timeout:?}");
}

fn terminate_worker(pid: &str) {
    #[cfg(unix)]
    let status = std::process::Command::new("kill")
        .args(["-9", pid])
        .status()
        .expect("terminate packaged worker");
    #[cfg(windows)]
    let status = std::process::Command::new("taskkill.exe")
        .args(["/PID", pid, "/T", "/F"])
        .status()
        .expect("terminate packaged worker");
    assert!(status.success(), "packaged worker could not be terminated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires exact production worker, native runtime directory, model pack, and private canary file"]
async fn packaged_worker_ingests_recovers_after_crash_and_reopens_offline() {
    let worker = required_path("PROCYON_SEMANTIC_PRODUCTION_WORKER");
    let native_directory = required_path("PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY");
    let model_pack = required_path("PROCYON_SEMANTIC_PRODUCTION_MODEL_PACK");
    let canaries = privacy_canaries();
    let runtime = tempfile::tempdir().expect("runtime directory");
    let data = tempfile::tempdir().expect("semantic data directory");
    let launch =
        ManagedWorkerLaunch::new(worker, data.path().to_owned(), native_directory, model_pack);
    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(launch.clone()));
    let connector = WorkerConnector::desktop_managed_resolved(runtime.path(), resolver)
        .with_startup_timeout(Duration::from_secs(30))
        .with_idle_timeout(Duration::from_secs(5));
    let client = connector.connect().await.expect("start packaged worker");
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);

    let content = canaries.values().cloned().collect::<Vec<_>>().join("\n");
    let job_id = client
        .ingest(
            "qualification-ingestion",
            IngestionScope::new("qualification-tenant", "qualification-library"),
            "qualification-document",
            BTreeMap::from([
                (
                    "occurrence_id".to_owned(),
                    "qualification-occurrence".to_owned(),
                ),
                ("source_id".to_owned(), "qualification-source".to_owned()),
                ("root_id".to_owned(), "qualification-root".to_owned()),
                ("source-name".to_owned(), canaries["filename-path"].clone()),
            ]),
            "text/plain",
            content.into_bytes(),
        )
        .await
        .expect("ingest privacy fixture through packaged worker");
    assert_eq!(
        wait_for_ingestion(&client, &job_id, Duration::from_secs(60)).await,
        IngestionState::Completed
    );
    assert!(
        client
            .query(
                "qualification-tenant",
                "qualification-library",
                &canaries["query"],
                5,
            )
            .await
            .expect("query packaged worker")
            .iter()
            .any(|result| result.document_id == "qualification-document")
    );

    let first_pid = std::fs::read_to_string(runtime.path().join("worker.pid"))
        .expect("read packaged worker pid");
    terminate_worker(first_pid.trim());
    drop(client);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let restarted = connector.connect().await.expect("restart packaged worker");
    let second_pid = std::fs::read_to_string(runtime.path().join("worker.pid"))
        .expect("read restarted worker pid");
    assert_ne!(first_pid.trim(), second_pid.trim());
    assert_eq!(restarted.health().await.unwrap(), WorkerHealth::Serving);
    assert!(
        restarted
            .query(
                "qualification-tenant",
                "qualification-library",
                &canaries["query"],
                5,
            )
            .await
            .expect("query recovered packaged worker")
            .iter()
            .any(|result| result.document_id == "qualification-document")
    );
    restarted.shutdown(Duration::from_secs(10)).await.unwrap();
    connector
        .wait_until_stopped(Duration::from_secs(12))
        .await
        .unwrap();
}

#[cfg(feature = "gemma-native")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the exact Gemma-enabled packaged worker and verified original model files"]
async fn packaged_gemma_worker_ingests_multimodal_sources_offline() {
    use image::{ImageBuffer, ImageFormat, Rgb};

    let original_files: BTreeMap<String, PathBuf> = serde_json::from_str(
        &std::env::var("PROCYON_GEMMA_PACKAGED_FILES").expect("verified original-file paths"),
    )
    .unwrap();
    let files = GemmaNativeFiles::from_original_files(&original_files).unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let launch = ManagedWorkerLaunch::new_gemma(
        required_path("PROCYON_SEMANTIC_PRODUCTION_WORKER"),
        data.path().to_owned(),
        required_path("PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY"),
        files,
        128,
        GemmaMedia {
            images: true,
            audio: true,
            video: true,
        },
    );
    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(launch.clone()));
    let connector = WorkerConnector::desktop_managed_resolved(runtime.path(), resolver)
        .with_startup_timeout(Duration::from_secs(30));
    let client = connector
        .connect()
        .await
        .expect("start packaged Gemma worker");
    assert_eq!(client.health().await.unwrap(), WorkerHealth::Serving);

    let image = ImageBuffer::from_fn(128, 96, |x, y| {
        Rgb([
            ((x * 2 + y) % 256) as u8,
            ((x + y * 2) % 256) as u8,
            ((x + y) % 256) as u8,
        ])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png).unwrap();
    for (name, media_type, content) in [
        ("image", "image/png", png.into_inner()),
        (
            "audio",
            "audio/mpeg",
            include_bytes!("fixtures/gemma-audio-440hz-44k.mp3").to_vec(),
        ),
        (
            "video",
            "video/mp4",
            include_bytes!("fixtures/gemma-video-2s.mp4").to_vec(),
        ),
    ] {
        let job_id = client
            .ingest(
                &format!("gemma-{name}"),
                IngestionScope::new("qualification-tenant", "qualification-library"),
                &format!("document-{name}"),
                BTreeMap::from([
                    ("occurrence_id".into(), format!("occurrence-{name}")),
                    ("source_id".into(), format!("source-{name}")),
                    ("root_id".into(), "qualification-root".into()),
                    ("title".into(), format!("Gemma {name}")),
                ]),
                media_type,
                content,
            )
            .await
            .expect("submit packaged media");
        let started = Instant::now();
        assert_eq!(
            wait_for_ingestion(&client, &job_id, Duration::from_secs(300)).await,
            IngestionState::Completed,
            "{name}"
        );
        println!(
            "Gemma {name} packaged ingestion completed in {:.1}s",
            started.elapsed().as_secs_f64()
        );
        let results = client
            .query(
                "qualification-tenant",
                "qualification-library",
                &format!("Gemma {name}"),
                10,
            )
            .await
            .expect("search packaged media");
        let result = results
            .iter()
            .find(|result| result.document_id == format!("document-{name}"))
            .expect("packaged media is retrievable");
        assert_eq!(result.metadata.get("media_type").unwrap(), media_type);
        if name == "video" {
            let provenance = result.metadata.get("semantic.provenance").unwrap();
            assert!(provenance.contains("sampledTimestampsMs"), "{provenance}");
            assert!(provenance.contains("1000"), "{provenance}");
        }
    }
    if std::env::var_os("PROCYON_GEMMA_QUALIFY_RESTART").is_some() {
        let first_pid = std::fs::read_to_string(runtime.path().join("worker.pid"))
            .expect("read installed Gemma worker pid");
        terminate_worker(first_pid.trim());
        drop(client);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let restarted = connector
            .connect()
            .await
            .expect("restart installed Gemma worker");
        let second_pid = std::fs::read_to_string(runtime.path().join("worker.pid"))
            .expect("read restarted Gemma worker pid");
        assert_ne!(first_pid.trim(), second_pid.trim());
        for name in ["image", "audio", "video"] {
            assert!(
                restarted
                    .query(
                        "qualification-tenant",
                        "qualification-library",
                        &format!("Gemma {name}"),
                        10,
                    )
                    .await
                    .expect("query recovered installed Gemma index")
                    .iter()
                    .any(|result| result.document_id == format!("document-{name}")),
                "{name} was not retrievable after worker restart"
            );
        }
        restarted.shutdown(Duration::from_secs(10)).await.unwrap();
    } else {
        client.shutdown(Duration::from_secs(10)).await.unwrap();
    }
    connector
        .wait_until_stopped(Duration::from_secs(12))
        .await
        .unwrap();
}
