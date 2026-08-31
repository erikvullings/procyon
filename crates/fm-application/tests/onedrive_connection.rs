//! Proves the `FileManagerService`-level OneDrive wiring end to end through
//! its public DTO API (task 0110): `fm-vfs-onedrive`'s
//! `OneDriveFileSystemProvider` is genuinely registered in the provider
//! registry (not merely present in source), a OneDrive connection can be
//! created without any secret (create-before-authorize, spec: "no secret
//! input should be required/accepted for normal OneDrive creation"), a raw
//! OAuth token can never be smuggled in through the generic connection
//! create/update surface, and `connect`/`test`/browsing all report the
//! distinct, actionable states an unauthorized connection should before
//! task 0110's deeper authorization-attempt flow (covered by
//! `fm-application`'s own internal `onedrive` module tests, which exercise
//! it against loopback OAuth/Graph fixtures) ever runs.

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    ConnectionConfigurationDto, ConnectionKindDto, ConnectionSecretInputDto, ConnectionStatusDto,
    CreateConnectionRequestDto, ListDirectoryRequest, OneDriveConnectionConfigurationDto,
    RuntimeKindDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn onedrive_configuration() -> ConnectionConfigurationDto {
    ConnectionConfigurationDto::OneDrive(OneDriveConnectionConfigurationDto::default())
}

#[tokio::test]
async fn a_onedrive_connection_can_be_created_without_any_secret() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);

    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKindDto::OneDrive,
            configuration: onedrive_configuration(),
            secret: None,
        })
        .await
        .expect("create-before-authorize must succeed with no secret");

    assert!(!created.has_credential);
    assert_eq!(created.status, ConnectionStatusDto::Disconnected);
    let ConnectionConfigurationDto::OneDrive(configuration) = created.configuration else {
        panic!("expected a OneDrive configuration");
    };
    assert_eq!(configuration.email, None);
    assert_eq!(configuration.display_name, None);
    assert_eq!(configuration.drive_type, None);
}

#[tokio::test]
async fn creating_a_onedrive_connection_with_a_raw_oauth_token_is_rejected() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);

    let error = service
        .create_connection(CreateConnectionRequestDto {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKindDto::OneDrive,
            configuration: onedrive_configuration(),
            secret: Some(ConnectionSecretInputDto::OAuthToken {
                access_token: "smuggled-access-token".to_owned(),
                refresh_token: Some("smuggled-refresh-token".to_owned()),
            }),
        })
        .await
        .expect_err("a raw OAuth token must never be accepted through connection create");

    let message = error.to_string();
    assert!(!message.contains("smuggled-access-token"));
    assert!(!message.contains("smuggled-refresh-token"));

    // Nothing was persisted.
    assert!(service.list_connections().await.unwrap().is_empty());
}

#[tokio::test]
async fn updating_a_onedrive_connection_with_a_raw_oauth_token_is_also_rejected() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKindDto::OneDrive,
            configuration: onedrive_configuration(),
            secret: None,
        })
        .await
        .unwrap();

    let error = service
        .update_connection(
            created.id,
            fm_transport_dto::UpdateConnectionRequestDto {
                name: created.name.clone(),
                kind: ConnectionKindDto::OneDrive,
                configuration: onedrive_configuration(),
                secret: Some(ConnectionSecretInputDto::OAuthToken {
                    access_token: "smuggled-token".to_owned(),
                    refresh_token: None,
                }),
            },
        )
        .await
        .expect_err("update must also reject a raw OAuth token");

    assert!(!error.to_string().contains("smuggled-token"));
}

#[tokio::test]
async fn connect_and_test_report_authentication_required_before_authorization() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKindDto::OneDrive,
            configuration: onedrive_configuration(),
            secret: None,
        })
        .await
        .unwrap();

    let connected = service.connect_connection(created.id).await.unwrap();
    assert_eq!(
        connected.status,
        ConnectionStatusDto::AuthenticationRequired
    );

    let tested = service.test_connection(created.id).await.unwrap();
    assert_eq!(tested.status, ConnectionStatusDto::AuthenticationRequired);
}

#[tokio::test]
async fn browsing_an_unauthorized_onedrive_location_reports_credential_required_proving_the_provider_is_registered()
 {
    // If `fm-vfs-onedrive`'s provider were *not* registered with the
    // `ProviderRegistry`, this would report `ProviderUnavailable` instead -
    // `CredentialRequired` can only come from the registered provider
    // actually being reached and asking its resolver for a token.
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let created = service
        .create_connection(CreateConnectionRequestDto {
            name: "My OneDrive".to_owned(),
            kind: ConnectionKindDto::OneDrive,
            configuration: onedrive_configuration(),
            secret: None,
        })
        .await
        .unwrap();
    let location = Location::parse(&format!("onedrive://{}/", created.id))
        .expect("valid onedrive root location");

    let error = service
        .list_directory(ListDirectoryRequest {
            workspace_id: fm_domain::WorkspaceId::new().into(),
            pane_id: fm_domain::PaneId::new().into(),
            request_id: uuid::Uuid::new_v4(),
            location: location.into(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: false,
        })
        .await
        .expect_err("browsing without a credential must fail");

    assert_eq!(error, fm_application::ApplicationError::CredentialRequired);
}

#[tokio::test]
async fn browsing_an_unknown_onedrive_connection_reports_not_found_not_provider_unavailable() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let location = Location::parse(&format!("onedrive://{}/", uuid::Uuid::new_v4()))
        .expect("valid onedrive root location for an id that was never saved");

    let error = service
        .list_directory(ListDirectoryRequest {
            workspace_id: fm_domain::WorkspaceId::new().into(),
            pane_id: fm_domain::PaneId::new().into(),
            request_id: uuid::Uuid::new_v4(),
            location: location.into(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: true,
            show_git_status: false,
        })
        .await
        .expect_err("browsing an unknown connection must fail");

    assert_eq!(error, fm_application::ApplicationError::NotFound);
}
