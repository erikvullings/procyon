//! Axum host for the file manager backend (spec §2.2, §8, §9, §21, §33 step 2).
//!
//! The SSE endpoint arrives in task 0032. Handlers stay thin: all behaviour
//! lives in `fm-application`.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use fm_server::config::{ServerConfig, ServerFileConfig};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Command line and environment configuration for the Axum host.
///
/// Precedence, highest to lowest: CLI flag/environment variable, then
/// `--config` file (spec §22, task 0064), then the built-in default.
#[derive(Parser, Debug)]
#[command(name = "fm-server", about = "File manager backend")]
struct Cli {
    /// Subcommand to run instead of serving (task 0009). Absent means "serve".
    #[command(subcommand)]
    command: Option<Command>,
    /// Path to a server-mode configuration file (TOML), separate from the
    /// desktop app's settings (task 0064).
    #[arg(long = "config", env = "FM_SERVER_CONFIG")]
    config: Option<PathBuf>,
    /// Address to bind to. Defaults to loopback (spec §22).
    #[arg(long, env = "FM_SERVER_BIND")]
    bind: Option<IpAddr>,
    /// Port to bind to.
    #[arg(long, env = "FM_SERVER_PORT")]
    port: Option<u16>,
    /// Origins allowed to make cross-origin requests. Repeat to allow several;
    /// omit to allow none (spec §22, no wildcard CORS).
    #[arg(
        long = "cors-origin",
        env = "FM_SERVER_CORS_ORIGIN",
        value_delimiter = ','
    )]
    cors_origin: Vec<String>,
    /// Filesystem roots the server is permitted to expose (task 0064).
    #[arg(long = "root", env = "FM_SERVER_ROOT", value_delimiter = ',')]
    root: Vec<PathBuf>,
    /// Maximum accepted request body size, in bytes (task 0064).
    #[arg(long = "max-body-bytes", env = "FM_SERVER_MAX_BODY_BYTES")]
    max_body_bytes: Option<usize>,
    /// Maximum mutating requests accepted per second, server-wide (task 0064).
    #[arg(
        long = "max-mutations-per-second",
        env = "FM_SERVER_MAX_MUTATIONS_PER_SECOND"
    )]
    max_mutations_per_second: Option<u32>,
    /// PEM certificate chain path for direct TLS termination. Requires
    /// `--tls-key` to also be set (task 0064).
    #[arg(long = "tls-cert", env = "FM_SERVER_TLS_CERT")]
    tls_cert: Option<PathBuf>,
    /// PEM private key path for direct TLS termination. Requires
    /// `--tls-cert` to also be set (task 0064).
    #[arg(long = "tls-key", env = "FM_SERVER_TLS_KEY")]
    tls_key: Option<PathBuf>,
    /// DEVELOPMENT ONLY: Disable authentication checks. Logged at startup
    /// and impossible when binding to non-loopback addresses (task 0064).
    #[arg(long, env = "FM_SERVER_DEV_MODE_AUTH_DISABLED")]
    dev_mode_auth_disabled: bool,
}

/// Subcommands that run instead of serving requests.
#[derive(Subcommand, Debug)]
enum Command {
    /// Writes the deterministic OpenAPI document to `path` and exits without
    /// binding a port (spec §9).
    ExportOpenapi {
        /// Output file path for the exported OpenAPI document.
        path: PathBuf,
    },
}

/// Resolved TLS material, if direct in-process TLS termination is enabled.
struct TlsPaths {
    cert: PathBuf,
    key: PathBuf,
}

/// Merges the CLI/env layer over an optional config-file layer into a
/// [`ServerConfig`], then validates the loopback/dev-mode invariant.
///
/// Returns the resolved config plus the optional TLS material, since TLS
/// paths aren't part of [`ServerConfig`] (only `main` uses them, to choose
/// between `axum::serve` and `axum_server`'s rustls acceptor).
fn resolve_config(cli: &Cli, file: Option<&ServerFileConfig>) -> (ServerConfig, Option<TlsPaths>) {
    let defaults = ServerConfig::default();

    let bind_address = cli
        .bind
        .or_else(|| file.and_then(|f| f.bind))
        .unwrap_or(defaults.bind_address);
    let port = cli
        .port
        .or_else(|| file.and_then(|f| f.port))
        .unwrap_or(defaults.port);
    let cors_allowed_origins = if !cli.cors_origin.is_empty() {
        cli.cors_origin.clone()
    } else {
        file.and_then(|f| f.cors_origins.clone())
            .unwrap_or(defaults.cors_allowed_origins)
    };
    let roots = if !cli.root.is_empty() {
        cli.root.clone()
    } else {
        file.and_then(|f| f.roots.clone()).unwrap_or(defaults.roots)
    };
    let max_body_bytes = cli
        .max_body_bytes
        .or_else(|| file.and_then(|f| f.max_body_bytes))
        .unwrap_or(defaults.max_body_bytes);
    let max_mutations_per_second = cli
        .max_mutations_per_second
        .or_else(|| file.and_then(|f| f.max_mutations_per_second))
        .unwrap_or(defaults.max_mutations_per_second);

    let is_loopback = matches!(
        bind_address,
        IpAddr::V4(addr) if addr.octets()[0] == 127,
    ) || matches!(bind_address, IpAddr::V6(addr) if addr.is_loopback());

    let dev_mode_auth_disabled =
        cli.dev_mode_auth_disabled || file.and_then(|f| f.dev_mode_auth_disabled).unwrap_or(false);
    if dev_mode_auth_disabled && !is_loopback {
        panic!("dev-mode auth disable is not allowed when binding to non-loopback addresses");
    }

    let tls_cert = cli
        .tls_cert
        .clone()
        .or_else(|| file.and_then(|f| f.tls_cert.clone()));
    let tls_key = cli
        .tls_key
        .clone()
        .or_else(|| file.and_then(|f| f.tls_key.clone()));
    let tls = match (tls_cert, tls_key) {
        (Some(cert), Some(key)) => Some(TlsPaths { cert, key }),
        (None, None) => None,
        _ => panic!("--tls-cert and --tls-key must both be set to enable direct TLS termination"),
    };

    let config = ServerConfig {
        bind_address,
        port,
        cors_allowed_origins,
        max_body_bytes,
        max_mutations_per_second,
        roots,
        dev_mode_auth_disabled,
        ..defaults
    };
    (config, tls)
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Some(Command::ExportOpenapi { path }) = &cli.command {
        fm_server::openapi_export::write_to_file(path).unwrap_or_else(|err| {
            panic!(
                "failed to export OpenAPI document to {}: {err}",
                path.display()
            )
        });
        println!("wrote OpenAPI document to {}", path.display());
        return;
    }

    init_tracing();

    let file_config = cli.config.as_deref().map(|path| {
        ServerFileConfig::load(path).unwrap_or_else(|err| {
            panic!("failed to load server config file: {err}");
        })
    });
    let (config, tls) = resolve_config(&cli, file_config.as_ref());
    let router = fm_server::build_router(&config);

    let is_loopback = matches!(
        config.bind_address,
        IpAddr::V4(addr) if addr.octets()[0] == 127,
    ) || matches!(config.bind_address, IpAddr::V6(addr) if addr.is_loopback());

    if !is_loopback {
        tracing::warn!(
            bind = %config.bind_address,
            "binding to non-loopback address; ensure TLS and authentication are configured"
        );
    }

    if config.dev_mode_auth_disabled {
        tracing::warn!("DEVELOPMENT MODE: authentication disabled; do not use in production");
    } else {
        let manager = fm_server::auth::SessionManager::new(config.session_secret.clone(), false);
        let token = manager.issue_token();
        println!(
            "fm-server access token (pass as `Authorization: Bearer <token>` or `?token=` on the SSE URL):\n{}",
            token.as_str()
        );
    }

    if config.roots.is_empty() {
        tracing::warn!("no accessible roots configured; server can access entire filesystem");
    }

    tracing::info!(
        bind = %config.bind_address,
        port = config.port,
        loopback_only = is_loopback,
        auth_required = !config.dev_mode_auth_disabled,
        num_roots = config.roots.len(),
        tls_enabled = tls.is_some(),
        "starting fm-server"
    );

    match tls {
        Some(TlsPaths { cert, key }) => {
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key)
                .await
                .unwrap_or_else(|err| panic!("failed to load TLS material: {err}"));
            axum_server::bind_rustls((config.bind_address, config.port).into(), tls_config)
                .serve(router.into_make_service())
                .await
                .expect("fm-server exited unexpectedly");
        }
        None => {
            let listener = TcpListener::bind((config.bind_address, config.port))
                .await
                .expect("failed to bind fm-server listener");
            axum::serve(listener, router)
                .await
                .expect("fm-server exited unexpectedly");
        }
    }
}

/// Initialises structured tracing.
///
/// - `RUST_LOG` controls the level filter (default: `info,notify::poll=error`).
/// - `FM_LOG_FORMAT` controls the output format: `compact` (default), `pretty`, or `json`.
/// - `FM_LOG_FILE` redirects output to a rolling daily log file at the given path prefix (spec §30).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,notify::poll=error"));

    let format = std::env::var("FM_LOG_FORMAT").unwrap_or_default();
    let log_file = std::env::var("FM_LOG_FILE").ok();

    match log_file {
        Some(path) => {
            let dir = std::path::Path::new(&path)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let prefix = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("fm-server");
            let file_appender = tracing_appender::rolling::daily(dir, prefix);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            // _guard must live for the program's lifetime; leak it intentionally.
            std::mem::forget(_guard);
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
                .init();
        }
        None => match format.as_str() {
            "pretty" => tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().pretty())
                .init(),
            _ => tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().compact())
                .init(),
        },
    }
}
