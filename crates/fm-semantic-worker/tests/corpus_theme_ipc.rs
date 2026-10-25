//! Corpus-theme IPC behavior over the real local transport.

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use fm_semantic_worker::corpus_theme_ipc::{
    ThemeFailure, ThemeRequest, ThemeResponse, WorkerCorpusThemeBackend,
};
use fm_semantic_worker::corpus_theme_storage::CorpusScope;
use fm_semantic_worker::corpus_themes::{
    CorpusThemeConfig, CorpusThemeError, CorpusThemeStatus, CorpusThemes, MapPoint, NoTheme,
    ThemePassage,
};
use fm_semantic_worker::{
    ClientError, Endpoint, LaunchSecret, WorkerClient, WorkerConfig, WorkerServer,
};
use tokio_util::sync::CancellationToken;

struct TestDirectory(tempfile::TempDir);

impl Deref for TestDirectory {
    type Target = std::path::Path;
    fn deref(&self) -> &Self::Target {
        self.0.path()
    }
}

fn test_directory() -> TestDirectory {
    TestDirectory(
        tempfile::Builder::new()
            .prefix("fm-themes-")
            .tempdir()
            .expect("create isolated worker test directory"),
    )
}

async fn wait_for_endpoint(endpoint: &Endpoint) {
    for _ in 0..200 {
        if endpoint.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("worker endpoint never appeared");
}

fn scope() -> CorpusScope {
    CorpusScope {
        tenant_id: "tenant".to_owned(),
        library_id: "library".to_owned(),
        source_ids: vec!["source-1".to_owned()],
    }
}

fn passage(index: usize) -> ThemePassage {
    ThemePassage {
        chunk_id: format!("chunk-{index}"),
        document_id: "document".to_owned(),
        source_id: "source-1".to_owned(),
        source_position: 0,
        section_path: Vec::new(),
        provenance: String::new(),
        excerpt: "x".repeat(240),
        distance: 0.0,
    }
}

fn themes_with_points(points: usize) -> CorpusThemes {
    CorpusThemes {
        algorithm_version: "test".to_owned(),
        model_tag: "model".to_owned(),
        index_revision: "revision".to_owned(),
        fingerprint: "fingerprint".to_owned(),
        chunk_count: points as u64,
        document_count: 1,
        sampled: false,
        fitted_chunks: 0,
        themes: Vec::new(),
        no_theme: NoTheme {
            chunk_count: 0,
            document_count: 0,
            share: 0.0,
            passages: Vec::new(),
        },
        document_mixtures: Vec::new(),
        mixtures_truncated: false,
        map_points: (0..points)
            .map(|index| MapPoint {
                x: 0.5,
                y: -0.5,
                theme: None,
                passage: passage(index),
            })
            .collect(),
        map_sampled: false,
        centroids_recomputed: 0,
        centroids_reused: 0,
        from_cache: false,
        incremental: false,
    }
}

struct StubBackend;

impl WorkerCorpusThemeBackend for StubBackend {
    fn handle(
        &self,
        request: ThemeRequest,
        _cancellation: &CancellationToken,
    ) -> Result<ThemeResponse, CorpusThemeError> {
        match request {
            ThemeRequest::Status { scope } => {
                assert_eq!(scope.source_ids, vec!["source-1".to_owned()]);
                Ok(ThemeResponse::Status(CorpusThemeStatus {
                    index_revision: "revision".to_owned(),
                    document_count: 1,
                    chunk_count: 7,
                    model_id: "model".to_owned(),
                    profile_too_weak: false,
                }))
            }
            ThemeRequest::Analyze { config, .. } if config.max_map_points == 1 => {
                Err(CorpusThemeError::InsufficientContent { chunks: 3 })
            }
            ThemeRequest::Analyze { .. } => {
                Ok(ThemeResponse::Themes(Box::new(themes_with_points(4_000))))
            }
            ThemeRequest::Sources { .. } => Err(CorpusThemeError::ThemesStale),
        }
    }
}

#[tokio::test]
async fn corpus_themes_round_trip_typed_results_failures_and_bound_the_map() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([53; 32]);
    let task = tokio::spawn(
        WorkerServer::with_corpus_theme_backend(
            WorkerConfig::new(endpoint.clone(), secret.clone()),
            Arc::new(StubBackend),
        )
        .run(),
    );
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();

    let status = client
        .corpus_themes("status-1", &ThemeRequest::Status { scope: scope() })
        .await
        .unwrap();
    assert!(matches!(status, ThemeResponse::Status(s) if s.chunk_count == 7));

    let failure = client
        .corpus_themes(
            "analyze-1",
            &ThemeRequest::Analyze {
                scope: scope(),
                config: CorpusThemeConfig {
                    max_map_points: 1,
                    ..CorpusThemeConfig::default()
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        failure,
        ThemeResponse::Failure(ThemeFailure::InsufficientContent { chunks: 3 })
    );

    let stale = client
        .corpus_themes(
            "sources-1",
            &ThemeRequest::Sources {
                scope: scope(),
                theme: Some(1),
                fingerprint: "old".to_owned(),
                limit: 10,
            },
        )
        .await
        .unwrap();
    assert_eq!(stale, ThemeResponse::Failure(ThemeFailure::ThemesStale));

    let analysed = client
        .corpus_themes(
            "analyze-2",
            &ThemeRequest::Analyze {
                scope: scope(),
                config: CorpusThemeConfig::default(),
            },
        )
        .await
        .unwrap();
    let ThemeResponse::Themes(themes) = analysed else {
        panic!("expected themes");
    };
    assert!(
        themes.map_sampled,
        "an oversized map is thinned and flagged"
    );
    assert!(!themes.map_points.is_empty() && themes.map_points.len() < 4_000);

    drop(client);
    task.abort();
}

#[tokio::test]
async fn a_worker_without_a_theme_backend_does_not_advertise_the_capability() {
    let directory = test_directory();
    let endpoint = Endpoint::for_runtime_directory(&directory);
    let secret = LaunchSecret::from_bytes([59; 32]);
    let task =
        tokio::spawn(WorkerServer::new(WorkerConfig::new(endpoint.clone(), secret.clone())).run());
    wait_for_endpoint(&endpoint).await;
    let client = WorkerClient::connect(&endpoint, secret).await.unwrap();
    assert!(matches!(
        client
            .corpus_themes("status", &ThemeRequest::Status { scope: scope() })
            .await,
        Err(ClientError::CapabilityUnavailable { .. })
    ));
    drop(client);
    task.abort();
}
