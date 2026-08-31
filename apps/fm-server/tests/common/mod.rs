//! Shared test helpers: spawns the real Axum host on an ephemeral port with
//! workspace storage isolated to a temp directory, so integration tests
//! never touch the developer's real platform config directory.

use std::net::{IpAddr, Ipv4Addr};

use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_server::config::ServerConfig;
use fm_transport_dto::RuntimeKindDto;
use std::sync::Arc;

/// A native path as a validated `file:` URI. Formatting the path into the URI
/// instead only produces a valid result for POSIX paths: a Windows path yields
/// `file://C:\dir`, which the domain layer rejects.
#[allow(dead_code)]
pub(crate) fn file_uri(path: &std::path::Path) -> String {
    fm_domain::Location::from_native_path(path)
        .expect("test fixture path must be an absolute native path")
        .uri
}

/// A spawned test server plus the `TempDir` its workspace storage lives in.
///
/// The `TempDir` must be kept alive for the duration of the test (dropping it
/// deletes the directory), so it is returned alongside the server handle
/// rather than being an internal detail.
pub(crate) struct TestServer {
    pub(crate) base_url: String,
    pub(crate) handle: tokio::task::JoinHandle<()>,
    #[allow(dead_code)]
    pub(crate) event_bus: EventBus,
    #[allow(dead_code)]
    pub(crate) session: fm_server::DevelopmentSession,
    _workspace_directory: tempfile::TempDir,
}

impl TestServer {
    #[allow(dead_code)]
    pub(crate) async fn spawn() -> Self {
        let workspace_directory =
            tempfile::tempdir().expect("must create a temp workspace directory");
        let config = ServerConfig {
            bind_address: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
            workspace_directory: workspace_directory.path().to_path_buf(),
            settings_directory: workspace_directory.path().join("config"),
            // These route tests exercise application behaviour, not the
            // auth/roots security surface (task 0064 covers that in
            // `tests/security.rs` with its own explicit configs), so the
            // shared spawn helper opts into the documented, logged dev-mode
            // relaxation rather than requiring every test to mint a token.
            dev_mode_auth_disabled: true,
            ..ServerConfig::default()
        };
        let event_bus = EventBus::new(8);
        let service = Arc::new(FileManagerService::with_event_bus(
            RuntimeKindDto::BrowserServer,
            config.workspace_directory.clone(),
            config.settings_directory.clone(),
            event_bus.clone(),
        ));
        Self::spawn_with_service(config, service, workspace_directory).await
    }

    pub(crate) async fn spawn_with_service(
        config: ServerConfig,
        service: Arc<FileManagerService>,
        workspace_directory: tempfile::TempDir,
    ) -> Self {
        let event_bus = service.event_bus().clone();
        let session = fm_server::DevelopmentSession::new();
        let router =
            fm_server::build_router_with_service_and_session(&config, service, session.clone());

        let listener = tokio::net::TcpListener::bind((config.bind_address, config.port))
            .await
            .expect("failed to bind an ephemeral port");
        let addr = listener
            .local_addr()
            .expect("bound listener must have a local address");

        let handle = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("test server exited unexpectedly");
        });

        Self {
            base_url: format!("http://{addr}"),
            handle,
            event_bus,
            session,
            _workspace_directory: workspace_directory,
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
