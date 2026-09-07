//! Integration coverage for managed semantic component REST routes (task 0178).

mod common;

use std::path::Path;
use std::sync::Arc;

use fm_application::FileManagerService;
use fm_server::config::ServerConfig;
use fm_transport_dto::{
    RuntimeKindDto, SemanticComponentAuthorityDto, SemanticComponentCapabilitiesDto,
    SemanticComponentErrorCodeDto, SemanticComponentErrorDetailsDto, SemanticComponentErrorDto,
    SemanticComponentLifecycleDto, SemanticComponentOperationDto, SemanticComponentStatusDto,
    SemanticModelProfileDto, SemanticRuntimeExecutableDownloadDto,
};
use reqwest::StatusCode;

use common::TestServer;

fn project_temp_dir(prefix: &str) -> tempfile::TempDir {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/server-component-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

#[tokio::test]
async fn status_is_available_for_administrator_provisioned_components() {
    let server = spawn_browser_server().await;

    let response = reqwest::get(format!(
        "{}/api/v1/semantic/components/status",
        server.base_url
    ))
    .await
    .expect("GET semantic component status");

    assert_eq!(response.status(), StatusCode::OK);
    let status: SemanticComponentStatusDto = response.json().await.expect("status DTO");
    assert_eq!(status.lifecycle, SemanticComponentLifecycleDto::Absent);
    assert_eq!(status.disk_use.categories.len(), 6);
}

#[tokio::test]
async fn profiles_are_readable_for_administrator_provisioned_components() {
    let server = spawn_browser_server().await;

    let response = reqwest::get(format!(
        "{}/api/v1/semantic/components/profiles",
        server.base_url
    ))
    .await
    .expect("GET semantic component profiles");

    assert_eq!(response.status(), StatusCode::OK);
    let profiles: Vec<SemanticModelProfileDto> = response.json().await.expect("profiles DTO");
    assert!(profiles.is_empty());
}

#[tokio::test]
async fn browser_cannot_create_an_installation_offer() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/installation-offers",
            server.base_url
        ))
        .json(&serde_json::json!({ "profile": "compactMultilingual" }))
        .send()
        .await
        .expect("POST semantic installation offer");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
    assert_eq!(error.code, SemanticComponentErrorCodeDto::AuthorityDenied);
    assert!(matches!(
        error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::CreateInstallationOffer,
            ..
        })
    ));
}

#[tokio::test]
async fn browser_cannot_accept_or_install_an_offer() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/installation-offers/accept",
            server.base_url
        ))
        .json(&serde_json::json!({ "offerId": "opaque-offer" }))
        .send()
        .await
        .expect("POST semantic installation consent");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
    assert!(matches!(
        error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::InstallOrEnable,
            ..
        })
    ));
}

#[tokio::test]
async fn browser_cannot_pause_semantic_indexing() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/indexing/pause",
            server.base_url
        ))
        .send()
        .await
        .expect("POST pause semantic indexing");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_cannot_resume_semantic_indexing() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/indexing/resume",
            server.base_url
        ))
        .send()
        .await
        .expect("POST resume semantic indexing");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_cannot_plan_or_confirm_semantic_index_removal() {
    let server = spawn_browser_server().await;
    let client = reqwest::Client::new();
    let plan_response = client
        .post(format!(
            "{}/api/v1/semantic/components/index-removal-plans",
            server.base_url
        ))
        .json(&serde_json::json!({ "enrolmentId": "../../never-inventoried" }))
        .send()
        .await
        .expect("POST semantic index removal plan");
    assert_eq!(plan_response.status(), StatusCode::FORBIDDEN);
    let plan_error: SemanticComponentErrorDto =
        plan_response.json().await.expect("semantic error DTO");
    assert!(matches!(
        plan_error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::RemoveIndex,
            ..
        })
    ));

    let confirm_response = client
        .post(format!(
            "{}/api/v1/semantic/components/index-removal-plans/confirm",
            server.base_url
        ))
        .json(&serde_json::json!({ "planId": "never-created" }))
        .send()
        .await
        .expect("POST semantic index removal confirmation");
    assert_eq!(confirm_response.status(), StatusCode::FORBIDDEN);
    let confirm_error: SemanticComponentErrorDto =
        confirm_response.json().await.expect("semantic error DTO");
    assert!(matches!(
        confirm_error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::RemoveIndex,
            ..
        })
    ));

    let legacy_response = client
        .post(format!(
            "{}/api/v1/semantic/components/indexes/remove",
            server.base_url
        ))
        .json(&serde_json::json!({ "enrolmentId": "enrolment-1" }))
        .send()
        .await
        .expect("POST removed legacy semantic index route");
    assert_eq!(legacy_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn browser_cannot_change_the_semantic_data_root() {
    let server = spawn_browser_server().await;
    let destination = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/server-component-tests")
        .join(format!("forbidden-destination-{}", uuid::Uuid::new_v4()));
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/data/move",
            server.base_url
        ))
        .json(&serde_json::json!({
            "destination": destination.to_string_lossy()
        }))
        .send()
        .await
        .expect("POST move semantic data");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        !destination.exists(),
        "browser path must be rejected before filesystem use"
    );
}

#[tokio::test]
async fn browser_cannot_uninstall_with_either_explicit_index_decision() {
    let server = spawn_browser_server().await;
    let client = reqwest::Client::new();

    for index_decision in ["retain", "delete"] {
        let response = client
            .post(format!(
                "{}/api/v1/semantic/components/uninstall",
                server.base_url
            ))
            .json(&serde_json::json!({ "indexDecision": index_decision }))
            .send()
            .await
            .expect("POST uninstall semantic components");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
        assert!(matches!(
            error.details,
            Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
                operation: SemanticComponentOperationDto::UninstallComponents,
                ..
            })
        ));
    }
}

#[tokio::test]
async fn browser_cannot_install_a_worker_patch() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/worker-patches/install",
            server.base_url
        ))
        .json(&serde_json::json!({ "componentId": "semantic.worker" }))
        .send()
        .await
        .expect("POST install semantic worker patch");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
    assert!(matches!(
        error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::InstallWorkerPatch,
            ..
        })
    ));
}

#[tokio::test]
async fn browser_rejects_local_model_import_before_using_the_path() {
    let server = spawn_browser_server().await;
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/server-component-tests")
        .join(format!("never-read-{}.model", uuid::Uuid::new_v4()));
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/models/import-local",
            server.base_url
        ))
        .json(&serde_json::json!({
            "sourcePath": source_path.to_string_lossy(),
            "modelId": "expert.model",
            "upstreamRevision": "revision-1",
            "licenseSpdx": "Apache-2.0",
            "licenseNotice": "Expert model",
            "tokenizer": "tokenizer-1",
            "dimensions": 384,
            "normalization": "unitLength",
            "runtimeComponentId": "semantic.runtime",
            "runtimeVersionRequirement": ">=1.0.0, <2.0.0",
            "languageCoverage": ["en"],
            "estimatedDiskBytes": 100,
            "estimatedRamBytes": 200,
            "profile": "compactEnglish",
            "estimate": {
                "documents": 10,
                "sourceBytes": 1000
            }
        }))
        .send()
        .await
        .expect("POST import local semantic model");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!source_path.exists());
}

#[tokio::test]
async fn browser_cannot_plan_a_model_migration() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/model-migrations/plan",
            server.base_url
        ))
        .json(&serde_json::json!({
            "profile": "multilingualQuality",
            "estimate": {
                "documents": 10,
                "sourceBytes": 1000
            }
        }))
        .send()
        .await
        .expect("POST plan semantic model migration");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_cannot_confirm_a_model_migration() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/model-migrations/confirm",
            server.base_url
        ))
        .json(&serde_json::json!({ "migrationId": "migration-1" }))
        .send()
        .await
        .expect("POST confirm semantic model migration");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn browser_cannot_checkpoint_a_model_migration() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/model-migrations/checkpoint",
            server.base_url
        ))
        .json(&serde_json::json!({
            "migrationId": "migration-1",
            "completedDocuments": 5,
            "resumeCursor": "cursor-5"
        }))
        .send()
        .await
        .expect("POST checkpoint semantic model migration");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
    assert!(matches!(
        error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::CheckpointModelMigration,
            ..
        })
    ));
}

#[tokio::test]
async fn browser_cannot_complete_a_model_migration() {
    let server = spawn_browser_server().await;
    let response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/semantic/components/model-migrations/complete",
            server.base_url
        ))
        .json(&serde_json::json!({ "migrationId": "migration-1" }))
        .send()
        .await
        .expect("POST complete semantic model migration");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let error: SemanticComponentErrorDto = response.json().await.expect("semantic error DTO");
    assert!(matches!(
        error.details,
        Some(SemanticComponentErrorDetailsDto::AuthorityDenied {
            operation: SemanticComponentOperationDto::CompleteModelMigration,
            ..
        })
    ));
}

async fn spawn_browser_server() -> TestServer {
    let directory = project_temp_dir("browser-");
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
    TestServer::spawn_with_service(config, service, directory).await
}

#[tokio::test]
async fn capabilities_report_administrator_provisioned_browser_authority() {
    let server = spawn_browser_server().await;

    let response = reqwest::get(format!(
        "{}/api/v1/semantic/components/capabilities",
        server.base_url
    ))
    .await
    .expect("GET semantic component capabilities");

    assert_eq!(response.status(), StatusCode::OK);
    let capabilities: SemanticComponentCapabilitiesDto =
        response.json().await.expect("capabilities DTO");
    assert_eq!(
        capabilities.authority,
        SemanticComponentAuthorityDto::AdministratorProvisioned
    );
    assert_eq!(
        capabilities.operations,
        vec![
            SemanticComponentOperationDto::ViewStatus,
            SemanticComponentOperationDto::ViewCatalog,
        ]
    );
    assert_eq!(
        capabilities.runtime_executable_download,
        SemanticRuntimeExecutableDownloadDto::AdministratorProvisioned
    );
}

#[test]
fn index_removal_openapi_uses_plan_and_confirmation_operation_ids() {
    let document = fm_server::openapi_document();
    let expected = [
        (
            "/api/v1/semantic/components/index-removal-plans",
            "createSemanticComponentIndexRemovalPlan",
        ),
        (
            "/api/v1/semantic/components/index-removal-plans/confirm",
            "confirmSemanticComponentIndexRemoval",
        ),
    ];

    for (path, operation_id) in expected {
        let item = document
            .paths
            .paths
            .get(path)
            .expect("path must be present");
        assert_eq!(
            item.post
                .as_ref()
                .and_then(|operation| operation.operation_id.as_deref()),
            Some(operation_id)
        );
    }
    assert!(
        !document
            .paths
            .paths
            .contains_key("/api/v1/semantic/components/indexes/remove")
    );
}
