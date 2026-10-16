//! The operator-opted-in, read-only desktop semantic-library attachment
//! (task 0250) is served only to authenticated callers and never writes to
//! the desktop profile or its shared stores.

#![allow(clippy::unwrap_used)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fm_application::FileManagerService;
use fm_application::semantic::{
    FakeSemanticCapability, LibraryId as WorkerLibraryId, SemanticOperationId, SemanticQuery,
    SemanticScope, TenantId,
};
use fm_application::semantic_library::{
    FixedSemanticEnrolmentEstimator, SemanticAccessContext, SemanticEnrolmentEstimate,
    SemanticEstimateCompleteness, SemanticFolderContext, SemanticLibraryConfiguration,
    SemanticLibraryService, SemanticRootIdentity,
};
use fm_domain::{Location, WorkspaceId};
use fm_server::auth::SessionManager;
use fm_server::config::ServerConfig;
use fm_transport_dto::{
    RuntimeKindDto, SemanticLibraryAuthorityDto, SemanticLibraryCapabilitiesDto,
};
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

fn project_temp_dir(prefix: &str) -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/server-attachment-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

#[tokio::test]
async fn attached_server_requires_a_token_and_never_writes_the_desktop_profile() {
    let directory = project_temp_dir("attached-");
    let profile = directory.path().join("procyon");
    std::fs::create_dir_all(&profile).unwrap();
    let profile = profile.canonicalize().unwrap();
    let config = ServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        workspace_directory: profile.join("workspaces"),
        settings_directory: profile.clone(),
        semantic_desktop_profile: Some(profile.clone()),
        ..ServerConfig::default()
    };
    assert!(config.semantic_desktop_attachment().unwrap().is_some());
    let token = SessionManager::new(config.session_secret.clone(), false).issue_token();
    let router = fm_server::build_router(&config);
    let listener = tokio::net::TcpListener::bind((config.bind_address, 0))
        .await
        .unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let capabilities_url = format!("{base_url}/api/v1/semantic/library/capabilities");

    let anonymous = client.get(&capabilities_url).send().await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    // With no desktop model or consent yet, the attachment is unavailable
    // rather than creating a library of its own.
    let response = client
        .get(&capabilities_url)
        .bearer_auth(token.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let capabilities: SemanticLibraryCapabilitiesDto = response.json().await.unwrap();
    assert_eq!(
        capabilities.authority,
        SemanticLibraryAuthorityDto::Unavailable
    );
    assert!(capabilities.operations.is_empty());

    let import = client
        .post(format!("{base_url}/api/v1/semantic/vocabularies/import"))
        .bearer_auth(token.as_str())
        .json(&serde_json::json!({ "skosJson": "{}" }))
        .send()
        .await
        .unwrap();
    assert_eq!(import.status(), StatusCode::FORBIDDEN);

    assert!(!profile.join("semantic-components").exists());
    assert!(!profile.join("semantic").exists());
    assert!(!profile.join("semantic-library-policy.json").exists());
    server.abort();
}

fn indexed_library(state: &Path, root: &Path, workspace_id: WorkspaceId) -> SemanticLibraryService {
    let library = SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::balanced(
            state.join("library-config"),
            state.join("semantic-data"),
            uuid::Uuid::from_u128(0x250),
            "fixture-model",
            "revision-1",
            384,
            "fixture-space",
        )
        .unwrap(),
        Arc::new(FixedSemanticEnrolmentEstimator::new(
            SemanticEnrolmentEstimate {
                completeness: SemanticEstimateCompleteness::Estimated,
                estimated_files: Some(1),
                estimated_source_bytes: Some(64),
                estimated_extracted_bytes: Some(64),
                estimated_vector_bytes: Some(64),
                skipped_reason_counts: Default::default(),
                missing_model_download_bytes: None,
                unavailable_reason: None,
                filesystem_identity: Some(SemanticRootIdentity::new("volume", "root")),
            },
        )),
    )
    .unwrap();
    let context =
        SemanticFolderContext::new(workspace_id, Location::from_native_path(root).unwrap());
    let preview = library
        .preview_enrolment(&SemanticAccessContext::Host, context.clone(), true)
        .unwrap();
    library
        .confirm_enrolment(
            &SemanticAccessContext::Host,
            &preview.confirmation_id,
            preview.policy_revision,
            &context,
        )
        .unwrap();
    library
}

async fn spawn_attached(
    profile: &Path,
    roots: Vec<PathBuf>,
    service: Arc<FileManagerService>,
) -> (String, String, tokio::task::JoinHandle<()>) {
    let config = ServerConfig {
        bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        port: 0,
        workspace_directory: profile.join("workspaces"),
        settings_directory: profile.to_path_buf(),
        semantic_desktop_profile: Some(profile.to_path_buf()),
        roots,
        ..ServerConfig::default()
    };
    let token = SessionManager::new(config.session_secret.clone(), false).issue_token();
    let router = fm_server::build_router_with_service_and_session(
        &config,
        service,
        fm_server::DevelopmentSession::new(),
    );
    let listener = tokio::net::TcpListener::bind((config.bind_address, 0))
        .await
        .unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base_url, token.as_str().to_owned(), handle)
}

/// Citation and evidence resolution never returns a target outside the
/// server's current accessible roots, including after the root is replaced.
#[tokio::test]
async fn citations_resolve_only_inside_the_current_accessible_roots() {
    // Outside the repository, whose `.gitignore` excludes `target/` from
    // indexing.
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let root = base.join("documents");
    let elsewhere = base.join("elsewhere");
    let profile = base.join("procyon");
    for path in [&root, &elsewhere, &profile] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(root.join("welcome.txt"), "The orchard has semantic apples.").unwrap();

    let workspace_id = WorkspaceId::from(uuid::Uuid::from_u128(0x2500));
    let library = indexed_library(&base.join("state"), &root, workspace_id);
    let root_id = library.status(&SemanticAccessContext::Host).unwrap().roots[0]
        .id
        .parse()
        .unwrap();
    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            base.join("workspaces"),
            base.join("settings"),
        )
        .with_semantic_library_service(library)
        .with_semantic_capability(Arc::new(FakeSemanticCapability::new())),
    );
    let report = service
        .semantic_reconcile_enrolled_root(
            &SemanticAccessContext::Host,
            root_id,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let results = service
        .semantic_query(SemanticQuery {
            scope: SemanticScope::new(
                TenantId::new(workspace_id.to_string()),
                WorkerLibraryId::new(report.library_id.clone()),
            ),
            request_id: SemanticOperationId::new("attached-citation"),
            text: "semantic".to_owned(),
            concept: None,
            maximum_results: 10,
        })
        .await
        .unwrap();
    let source_id = results[0].metadata.get("source_id").unwrap().clone();
    let body = serde_json::json!({
        "workspaceId": workspace_id.into_inner(),
        "sourceId": source_id,
    });
    let client = reqwest::Client::new();
    let resolve = |base_url: &str, token: &str, route: &str| {
        client
            .post(format!("{base_url}{route}"))
            .bearer_auth(token)
            .json(&body)
            .send()
    };
    const ROUTES: [&str; 2] = [
        "/api/v1/semantic/ask/citations/resolve",
        "/api/v1/semantic/knowledge/sources/resolve",
    ];

    let (covering, covering_token, covering_handle) =
        spawn_attached(&profile, vec![root.clone()], service.clone()).await;
    for route in ROUTES {
        let response = resolve(&covering, &covering_token, route).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{route}");
        let resolved: serde_json::Value = response.json().await.unwrap();
        assert!(
            resolved["location"]["uri"]
                .as_str()
                .unwrap()
                .ends_with("/welcome.txt"),
            "{resolved}"
        );
    }

    let (restricted, restricted_token, restricted_handle) =
        spawn_attached(&profile, vec![elsewhere.clone()], service.clone()).await;
    for route in ROUTES {
        let response = resolve(&restricted, &restricted_token, route)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route}");
    }

    // The enrolled path now resolves outside the roots the server allows.
    #[cfg(unix)]
    {
        std::fs::rename(&root, base.join("original")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("welcome.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), &root).unwrap();
        let (relinked, relinked_token, relinked_handle) =
            spawn_attached(&profile, vec![base.join("original")], service.clone()).await;
        for route in ROUTES {
            let response = resolve(&relinked, &relinked_token, route).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{route}");
        }
        relinked_handle.abort();
    }
    covering_handle.abort();
    restricted_handle.abort();
}
