//! Cross-platform qualification of the exact packaged worker/runtime/model boundary.

#![cfg(feature = "semantic-runtime")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
) -> IngestionState {
    for _ in 0..600 {
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
    panic!("packaged ingestion did not finish within 60 seconds");
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
            BTreeMap::from([("source-name".to_owned(), canaries["filename-path"].clone())]),
            "text/plain",
            content.into_bytes(),
        )
        .await
        .expect("ingest privacy fixture through packaged worker");
    assert_eq!(
        wait_for_ingestion(&client, &job_id).await,
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
