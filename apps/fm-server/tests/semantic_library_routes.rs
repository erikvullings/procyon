//! HTTP parity and authorization coverage for semantic-library enrolment.

#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;

use fm_application::FileManagerService;
use fm_application::semantic_library::{
    SemanticLibraryConfiguration, SemanticLibraryService, SemanticServerIdentity,
    UnavailableSemanticEnrolmentEstimator,
};
use fm_server::config::ServerConfig;
use fm_transport_dto::{
    RuntimeKindDto, SemanticEnrolmentPreviewDto, SemanticFolderStatusDto,
    SemanticLibraryCapabilitiesDto, SemanticLibraryErrorCodeDto, SemanticLibraryErrorDto,
    SemanticLibraryStatusDto,
};
use reqwest::StatusCode;

use common::TestServer;

fn project_temp_dir(prefix: &str) -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/server-library-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

async fn spawn_mock_server() -> (TestServer, serde_json::Value) {
    let directory = project_temp_dir("mock-");
    let config = ServerConfig {
        port: 0,
        workspace_directory: directory.path().join("workspaces"),
        settings_directory: directory.path().join("settings"),
        dev_mode_auth_disabled: true,
        ..ServerConfig::default()
    };
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        &config.workspace_directory,
        &config.settings_directory,
    );
    let workspace = service
        .start_workspace(None)
        .await
        .expect("start mock workspace");
    let active_pane = workspace
        .panes
        .iter()
        .find(|pane| pane.id == workspace.active_pane_id)
        .expect("active pane");
    let active_tab = active_pane
        .tabs
        .iter()
        .find(|tab| tab.id == active_pane.active_tab_id)
        .expect("active tab");
    let context = serde_json::json!({
        "workspaceId": workspace.id,
        "location": active_tab.location,
    });
    (
        TestServer::spawn_with_service(config, Arc::new(service), directory).await,
        context,
    )
}

#[tokio::test]
async fn mock_http_enrolment_round_trip_has_folder_status_and_stale_revision_protection() {
    let (server, context) = spawn_mock_server().await;
    let client = reqwest::Client::new();

    let capabilities: SemanticLibraryCapabilitiesDto = client
        .get(format!(
            "{}/api/v1/semantic/library/capabilities",
            server.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        capabilities.authority,
        fm_transport_dto::SemanticLibraryAuthorityDto::DeterministicMock
    );

    let preview: SemanticEnrolmentPreviewDto = client
        .post(format!(
            "{}/api/v1/semantic/library/enrolment/preview",
            server.base_url
        ))
        .json(&serde_json::json!({
            "workspaceId": context["workspaceId"],
            "location": context["location"],
            "recursive": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        preview.estimate.completeness,
        fm_transport_dto::SemanticEstimateCompletenessDto::Partial
    );
    assert!(preview.normalized_excerpts_retained_locally);

    let status: SemanticLibraryStatusDto = client
        .post(format!(
            "{}/api/v1/semantic/library/enrolment/confirm",
            server.base_url
        ))
        .json(&serde_json::json!({
            "confirmationId": preview.confirmation_id,
            "policyRevision": preview.policy_revision,
            "workspaceId": context["workspaceId"],
            "location": context["location"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status.roots.len(), 1);

    let folder: SemanticFolderStatusDto = client
        .post(format!(
            "{}/api/v1/semantic/library/folder-status",
            server.base_url
        ))
        .json(&context)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        folder.consent,
        fm_transport_dto::SemanticFolderConsentDto::IncludedHere
    );
    assert!(folder.workspace_referenced);

    let response = client
        .post(format!("{}/api/v1/semantic/library/pause", server.base_url))
        .json(&serde_json::json!({"policyRevision": preview.policy_revision}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let error: SemanticLibraryErrorDto = response.json().await.unwrap();
    assert_eq!(error.code, SemanticLibraryErrorCodeDto::StaleRevision);
}

#[tokio::test]
async fn browser_server_denies_mutation_before_using_untrusted_location() {
    let directory = project_temp_dir("browser-deny-");
    let untrusted_path = directory.path().join("never-read");
    let untrusted_location =
        fm_domain::Location::from_native_path(&untrusted_path).expect("absolute test location");
    let config = ServerConfig {
        port: 0,
        workspace_directory: directory.path().join("workspaces"),
        settings_directory: directory.path().join("settings"),
        dev_mode_auth_disabled: true,
        ..ServerConfig::default()
    };
    let service = Arc::new(FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        &config.workspace_directory,
        &config.settings_directory,
    ));
    let server = TestServer::spawn_with_service(config, service, directory).await;

    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/library/enrolment/preview",
            server.base_url
        ))
        .json(&serde_json::json!({
            "workspaceId": uuid::Uuid::nil(),
            "location": {
                "providerId": "local",
                "uri": untrusted_location.uri
            },
            "recursive": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticLibraryErrorDto = response.json().await.unwrap();
    assert_eq!(error.code, SemanticLibraryErrorCodeDto::AuthorityDenied);
    assert!(!untrusted_path.exists());
}

#[tokio::test]
async fn exclusion_confirmation_rejects_caller_supplied_counts() {
    let (server, context) = spawn_mock_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/library/exclusions/confirm",
            server.base_url
        ))
        .json(&serde_json::json!({
            "confirmationId": "opaque",
            "policyRevision": 1,
            "workspaceId": context["workspaceId"],
            "location": context["location"],
            "categories": [{"category": "occurrences", "totalItems": 0}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn semantic_library_openapi_contains_parity_operations() {
    let document = fm_server::openapi_document();
    for (path, operation_id) in [
        (
            "/api/v1/semantic/library/folder-status",
            "getSemanticFolderStatus",
        ),
        (
            "/api/v1/semantic/library/enrolment/preview",
            "previewSemanticEnrolment",
        ),
        (
            "/api/v1/semantic/library/enrolment/confirm",
            "confirmSemanticEnrolment",
        ),
        (
            "/api/v1/semantic/library/exclusions/plan",
            "planSemanticExclusion",
        ),
        (
            "/api/v1/semantic/library/exclusions/confirm",
            "confirmSemanticExclusion",
        ),
    ] {
        let item = document.paths.paths.get(path).expect("path");
        assert_eq!(
            item.post
                .as_ref()
                .and_then(|operation| operation.operation_id.as_deref()),
            Some(operation_id)
        );
    }
}

/// Builds one read-only private server library owned by `private/administrator`.
fn administrator_library(config_root: &Path, data_root: &Path) -> SemanticLibraryService {
    SemanticLibraryService::server_single_private_read_only(
        SemanticLibraryConfiguration::balanced(
            config_root,
            data_root,
            uuid::Uuid::from_u128(0x179),
            "admin-model",
            "revision-1",
            384,
            "space-1",
        )
        .unwrap(),
        SemanticServerIdentity::new("private", "administrator").unwrap(),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
}

fn principal_config(directory: &tempfile::TempDir, tenant: &str, user: &str) -> ServerConfig {
    ServerConfig {
        port: 0,
        workspace_directory: directory.path().join("workspaces"),
        settings_directory: directory.path().join("settings"),
        dev_mode_auth_disabled: true,
        semantic_library_tenant_id: tenant.to_owned(),
        semantic_library_user_id: user.to_owned(),
        ..ServerConfig::default()
    }
}

#[tokio::test]
async fn one_service_instance_serves_its_administrator_and_denies_every_other_principal() {
    let directory = project_temp_dir("principal-");
    let config_root = directory.path().join("semantic-config");
    let data_root = directory.path().join("semantic-data");
    // One service instance, shared by both routers: the caller's authority
    // comes from trusted per-router server state, never from the service.
    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            directory.path().join("workspaces"),
            directory.path().join("settings"),
        )
        .with_semantic_library_service(administrator_library(&config_root, &data_root)),
    );
    let administrator_directory = project_temp_dir("principal-admin-");
    let denied_directory = project_temp_dir("principal-denied-");
    let administrator = TestServer::spawn_with_service(
        principal_config(&administrator_directory, "private", "administrator"),
        Arc::clone(&service),
        administrator_directory,
    )
    .await;
    let denied = TestServer::spawn_with_service(
        principal_config(&denied_directory, "private", "other-user"),
        Arc::clone(&service),
        denied_directory,
    )
    .await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!(
            "{}/api/v1/semantic/library/status",
            administrator.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let status: SemanticLibraryStatusDto = response.json().await.unwrap();
    assert_eq!(
        status.library.expect("configured library").library_id,
        "00000000-0000-0000-0000-000000000179"
    );

    let response = client
        .get(format!(
            "{}/api/v1/semantic/library/status",
            denied.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticLibraryErrorDto = response.json().await.unwrap();
    assert_eq!(error.code, SemanticLibraryErrorCodeDto::AccessDenied);

    // Capabilities are resolved per call from the same shared service, so the
    // denied router advertises no operations at all while the administrator
    // router still sees the read-only server surface.
    let administrator_capabilities: SemanticLibraryCapabilitiesDto = client
        .get(format!(
            "{}/api/v1/semantic/library/capabilities",
            administrator.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        administrator_capabilities.operations,
        vec![
            fm_transport_dto::SemanticLibraryOperationDto::ViewStatus,
            fm_transport_dto::SemanticLibraryOperationDto::ViewFolderStatus,
        ]
    );
    let denied_capabilities: SemanticLibraryCapabilitiesDto = client
        .get(format!(
            "{}/api/v1/semantic/library/capabilities",
            denied.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        denied_capabilities.authority,
        fm_transport_dto::SemanticLibraryAuthorityDto::AdministratorProvisioned
    );
    assert!(
        denied_capabilities.operations.is_empty(),
        "a denied principal must not be advertised administrator operations"
    );

    // Crafted headers cannot promote the denied principal.
    for (header, value) in [
        ("x-semantic-user", "administrator"),
        ("x-semantic-tenant", "private"),
        ("x-user-id", "administrator"),
        ("x-forwarded-user", "administrator"),
        ("authorization", "Bearer administrator"),
    ] {
        let response = client
            .get(format!(
                "{}/api/v1/semantic/library/status",
                denied.base_url
            ))
            .header(header, value)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "header {header} must not select a principal"
        );
    }

    // Nor can query parameters.
    let response = client
        .post(format!(
            "{}/api/v1/semantic/library/folder-status?tenantId=private&userId=administrator",
            denied.base_url
        ))
        .json(&serde_json::json!({
            "workspaceId": uuid::Uuid::nil(),
            "location": {"providerId": "local", "uri": "file:///never-read"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // There is no identity channel in the payload at all: an attempt to add one
    // fails schema validation before any handler runs.
    let response = client
        .post(format!(
            "{}/api/v1/semantic/library/folder-status",
            denied.base_url
        ))
        .json(&serde_json::json!({
            "workspaceId": uuid::Uuid::nil(),
            "location": {"providerId": "local", "uri": "file:///never-read"},
            "tenantId": "private",
            "userId": "administrator",
            "libraryId": "00000000-0000-0000-0000-000000000179"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // The administrator router is unaffected by a body naming another tenant:
    // it is still served as the configured administrator, so the failure is a
    // workspace conflict rather than an authorization decision.
    let response = client
        .post(format!(
            "{}/api/v1/semantic/library/folder-status?tenantId=other-tenant&userId=other-user",
            administrator.base_url
        ))
        .json(&serde_json::json!({
            "workspaceId": uuid::Uuid::nil(),
            "location": {"providerId": "local", "uri": "file:///never-read"}
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), StatusCode::FORBIDDEN);

    // No denied request may have created semantic storage. The administrator's
    // authorized read materialises the cross-process lock and nothing else.
    assert!(!config_root.exists());
    let mut entries: Vec<String> = std::fs::read_dir(&data_root)
        .expect("the authorized read must have created the data root")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(entries, vec!["library.lock".to_owned()]);
}

#[tokio::test]
async fn a_server_without_a_matching_principal_denies_every_semantic_call() {
    let directory = project_temp_dir("principal-invalid-");
    let config_root = directory.path().join("semantic-config");
    let data_root = directory.path().join("semantic-data");
    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::BrowserServer,
            directory.path().join("workspaces"),
            directory.path().join("settings"),
        )
        .with_semantic_library_service(administrator_library(&config_root, &data_root)),
    );
    let server_directory = project_temp_dir("principal-invalid-server-");
    // An empty configured identity cannot be constructed, so the server falls
    // back to an anonymous principal rather than to the administrator.
    let server = TestServer::spawn_with_service(
        principal_config(&server_directory, "", ""),
        service,
        server_directory,
    )
    .await;

    let response = reqwest::get(format!(
        "{}/api/v1/semantic/library/status",
        server.base_url
    ))
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticLibraryErrorDto = response.json().await.unwrap();
    assert_eq!(error.code, SemanticLibraryErrorCodeDto::AccessDenied);
    assert!(!config_root.exists());
    assert!(!data_root.exists());
}
