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
#[cfg(feature = "gemma-native")]
use serde_json::json;

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

#[cfg(all(feature = "gemma-native", not(target_os = "macos")))]
fn peak_worker_rss_bytes(pid: &str) -> u64 {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .expect("Linux worker high-water RSS");
        value
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            * 1024
    }
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("(Get-Process -Id {pid} -ErrorAction Stop).PeakWorkingSet64"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Windows worker peak working set unavailable"
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }
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

    let report_path = std::env::var_os("PROCYON_GEMMA_MEASURE_REPORT").map(PathBuf::from);
    assert!(
        report_path.is_none() || std::env::var_os("PROCYON_GEMMA_QUALIFY_RESTART").is_some(),
        "measurement requires the existing worker-restart check"
    );
    let dimensions = if report_path.is_some() {
        std::env::var("PROCYON_GEMMA_MEASURE_DIMENSIONS")
            .expect("measurement dimension is required")
            .parse::<usize>()
            .expect("measurement dimension must be numeric")
    } else {
        128
    };
    assert!([128, 256, 512, 768].contains(&dimensions));
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
        dimensions,
        GemmaMedia {
            images: true,
            audio: true,
            video: true,
        },
    );
    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(launch.clone()));
    let connector = WorkerConnector::desktop_managed_resolved(runtime.path(), resolver)
        .with_startup_timeout(Duration::from_secs(30));
    #[cfg(target_os = "macos")]
    let sampler = report_path.as_ref().map(|_| {
        let pid_path = runtime.path().join("worker.pid");
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = Arc::clone(&stop);
        let task = tokio::spawn(async move {
            let mut maximum = 0;
            while !finished.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(pid) = std::fs::read_to_string(&pid_path) {
                    let output = std::process::Command::new("ps")
                        .args(["-o", "rss=", "-p", pid.trim()])
                        .output()
                        .expect("sample macOS worker RSS");
                    if output.status.success() {
                        maximum = maximum.max(
                            String::from_utf8(output.stdout)
                                .unwrap()
                                .trim()
                                .parse::<u64>()
                                .expect("macOS worker RSS is numeric")
                                * 1024,
                        );
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            maximum
        });
        (stop, task)
    });
    let started = Instant::now();
    let client = connector
        .connect()
        .await
        .expect("start packaged Gemma worker");
    let startup_seconds = started.elapsed().as_secs_f64();
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
    let mut measurements = Vec::new();
    let mut sources = vec![
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
    ];
    if report_path.is_some() {
        sources.push((
            "text",
            "text/plain",
            b"Gemma text qualification paragraph describing filesystem navigation.".to_vec(),
        ));
    }
    for (name, media_type, content) in sources {
        let bytes = content.len();
        let started = Instant::now();
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
        assert_eq!(
            wait_for_ingestion(&client, &job_id, Duration::from_secs(300)).await,
            IngestionState::Completed,
            "{name}"
        );
        println!(
            "Gemma {name} packaged ingestion completed in {:.1}s",
            started.elapsed().as_secs_f64()
        );
        let ingestion_seconds = started.elapsed().as_secs_f64();
        let mut query_seconds = Vec::new();
        for _ in 0..if report_path.is_some() { 3 } else { 1 } {
            let queried = Instant::now();
            let results = client
                .query(
                    "qualification-tenant",
                    "qualification-library",
                    &format!("Gemma {name}"),
                    10,
                )
                .await
                .expect("search packaged media");
            query_seconds.push(queried.elapsed().as_secs_f64());
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
        measurements.push(json!({
            "modality": name,
            "inputBytes": bytes,
            "ingestionSeconds": ingestion_seconds,
            "ingestionItemsPerSecond": 1.0 / ingestion_seconds,
            "querySeconds": query_seconds,
        }));
    }
    let first_pid = std::fs::read_to_string(runtime.path().join("worker.pid"))
        .expect("read installed Gemma worker pid");
    #[cfg(not(target_os = "macos"))]
    let first_peak = report_path
        .as_ref()
        .map(|_| peak_worker_rss_bytes(first_pid.trim()));
    if std::env::var_os("PROCYON_GEMMA_QUALIFY_RESTART").is_some() {
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
        for name in if report_path.is_some() {
            &["image", "audio", "video", "text"][..]
        } else {
            &["image", "audio", "video"][..]
        } {
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
        #[cfg(not(target_os = "macos"))]
        let second_peak = report_path
            .as_ref()
            .map(|_| peak_worker_rss_bytes(second_pid.trim()));
        if let Some(path) = &report_path {
            #[cfg(target_os = "macos")]
            let (peak_rss_bytes, peak_method) = {
                let (stop, task) = sampler.expect("macOS RSS sampler");
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                (
                    task.await.expect("macOS RSS sampling task"),
                    "200ms ps RSS samples; startup/transient peaks may be missed",
                )
            };
            #[cfg(target_os = "linux")]
            let (peak_rss_bytes, peak_method) = (
                first_peak.unwrap().max(second_peak.unwrap()),
                "kernel VmHWM for both worker processes",
            );
            #[cfg(target_os = "windows")]
            let (peak_rss_bytes, peak_method) = (
                first_peak.unwrap().max(second_peak.unwrap()),
                "PeakWorkingSet64 for both worker processes",
            );
            assert!(peak_rss_bytes > 0, "worker peak RSS was not observed");
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&json!({
                    "schemaVersion": 1,
                    "dimensions": dimensions,
                    "totalWallSeconds": started.elapsed().as_secs_f64(),
                    "workerStartupSeconds": startup_seconds,
                    "workerPeakRssBytes": peak_rss_bytes,
                    "workerPeakMethod": peak_method,
                    "measurements": measurements,
                    "workload": "one synthetic 128x96 PNG, one 440Hz MP3, one 2s H.264 MP4, one short plain-text document; three text queries per modality; fresh worker and index per dimension",
                }))
                .unwrap(),
            )
            .unwrap();
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
