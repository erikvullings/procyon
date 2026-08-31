//! Typed server configuration (spec §22, §33 step 2, task 0064).
//!
//! Kept independent of the CLI parser in `main.rs` so integration tests and
//! future hosts (task 0064) can construct it directly.

use serde::Deserialize;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Random session secret per run; persisted only where deployment configures it.
/// In default config, this is in-memory only and changes per server restart.
#[derive(Debug, Clone)]
pub struct SessionSecret([u8; 32]);

impl SessionSecret {
    /// Generates a random session secret suitable for authentication.
    pub fn random() -> Self {
        // Generate 4 UUIDs and use their bytes as randomness
        let mut bytes = [0u8; 32];
        let uuid1 = Uuid::new_v4();
        let uuid2 = Uuid::new_v4();
        let uuid3 = Uuid::new_v4();
        let uuid4 = Uuid::new_v4();

        bytes[0..16].copy_from_slice(uuid1.as_bytes());
        bytes[16..32].copy_from_slice(uuid2.as_bytes());

        // Add additional entropy by XORing with other UUIDs
        for i in 0..16 {
            bytes[i] ^= uuid3.as_bytes()[i];
            bytes[i + 16] ^= uuid4.as_bytes()[i];
        }

        Self(bytes)
    }

    /// Returns the secret as a byte slice for use in authentication operations.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Runtime configuration for the Axum host.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind to; defaults to loopback so the server is never
    /// reachable from the network without explicit opt-in (spec §22).
    pub bind_address: IpAddr,
    /// TCP port to bind to. Use `0` to let the OS choose an ephemeral port.
    pub port: u16,
    /// Origins allowed to make cross-origin requests. Empty means no
    /// cross-origin requests are allowed; a wildcard is never accepted (spec
    /// §22).
    pub cors_allowed_origins: Vec<String>,
    /// Maximum accepted request body size, in bytes.
    pub max_body_bytes: usize,
    /// Maximum mutating (`POST`/`PUT`/`PATCH`/`DELETE`) requests accepted per
    /// second, server-wide, before `429 Too Many Requests` is returned
    /// (spec §22, task 0064).
    pub max_mutations_per_second: u32,
    /// Filesystem roots the server is permitted to expose. Validated after
    /// symlink resolution; all incoming Locations must resolve within one of
    /// these roots (task 0064).
    pub roots: Vec<PathBuf>,
    /// Directory workspaces are persisted under (spec §5.3.8).
    pub workspace_directory: PathBuf,
    /// Directory containing the application-wide settings file.
    pub settings_directory: PathBuf,
    /// Random session secret for authentication (task 0064).
    pub session_secret: SessionSecret,
    /// Whether to relax authentication in development mode. Explicit opt-in,
    /// logged at startup, and impossible when binding to non-loopback addresses
    /// (task 0064).
    pub dev_mode_auth_disabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: IpAddr::from([127, 0, 0, 1]),
            port: 8787,
            cors_allowed_origins: Vec::new(),
            max_body_bytes: 10 * 1024 * 1024,
            max_mutations_per_second: 20,
            roots: Vec::new(),
            workspace_directory:
                fm_application::workspace::JsonFileWorkspaceRepository::default_directory(),
            settings_directory: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from(".fm-config"))
                .join("fm"),
            session_secret: SessionSecret::random(),
            dev_mode_auth_disabled: false,
        }
    }
}

/// Server-mode configuration file, kept as its own TOML section/format
/// separate from the desktop app's settings (task 0064). Every field is
/// optional so a file only needs to set what it wants to override; CLI
/// flags and environment variables always take precedence over the file
/// (applied in `main.rs`).
///
/// ```toml
/// bind = "0.0.0.0"
/// port = 8787
/// cors_origins = ["https://files.example.com"]
/// roots = ["/home/user/documents", "/mnt/shared/public"]
/// max_body_bytes = 10485760
/// max_mutations_per_second = 20
/// dev_mode_auth_disabled = false
/// tls_cert = "/etc/fm-server/cert.pem"
/// tls_key = "/etc/fm-server/key.pem"
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerFileConfig {
    /// See [`ServerConfig::bind_address`].
    pub bind: Option<IpAddr>,
    /// See [`ServerConfig::port`].
    pub port: Option<u16>,
    /// See [`ServerConfig::cors_allowed_origins`].
    #[serde(default)]
    pub cors_origins: Option<Vec<String>>,
    /// See [`ServerConfig::roots`].
    #[serde(default)]
    pub roots: Option<Vec<PathBuf>>,
    /// See [`ServerConfig::max_body_bytes`].
    pub max_body_bytes: Option<usize>,
    /// See [`ServerConfig::max_mutations_per_second`].
    pub max_mutations_per_second: Option<u32>,
    /// See [`ServerConfig::dev_mode_auth_disabled`].
    pub dev_mode_auth_disabled: Option<bool>,
    /// PEM certificate chain path for direct TLS termination (task 0064).
    /// Requires `tls_key` to also be set.
    pub tls_cert: Option<PathBuf>,
    /// PEM private key path for direct TLS termination (task 0064). Requires
    /// `tls_cert` to also be set.
    pub tls_key: Option<PathBuf>,
}

/// A failure parsing a [`ServerFileConfig`] from disk.
#[derive(Debug, thiserror::Error)]
pub enum ServerFileConfigError {
    /// The file could not be read.
    #[error("failed to read server config file {path}: {source}")]
    Read {
        /// The file path that failed to read.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file's contents were not valid TOML matching [`ServerFileConfig`].
    #[error("failed to parse server config file {path}: {source}")]
    Parse {
        /// The file path that failed to parse.
        path: String,
        /// Underlying TOML error.
        #[source]
        source: toml::de::Error,
    },
}

impl ServerFileConfig {
    /// Loads and parses a server-mode configuration file from `path`.
    pub fn load(path: &Path) -> Result<Self, ServerFileConfigError> {
        let contents =
            std::fs::read_to_string(path).map_err(|source| ServerFileConfigError::Read {
                path: path.display().to_string(),
                source,
            })?;
        toml::from_str(&contents).map_err(|source| ServerFileConfigError::Parse {
            path: path.display().to_string(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_matches_the_browser_frontend_proxy() {
        assert_eq!(ServerConfig::default().port, 8787);
    }

    #[test]
    fn default_config_uses_loopback_binding() {
        let config = ServerConfig::default();
        assert_eq!(config.bind_address, IpAddr::from([127, 0, 0, 1]));
    }

    #[test]
    fn default_config_disables_auth_disabled() {
        let config = ServerConfig::default();
        assert!(!config.dev_mode_auth_disabled);
    }

    #[test]
    fn session_secret_generates_different_values() {
        let secret1 = SessionSecret::random();
        let secret2 = SessionSecret::random();
        assert_ne!(secret1.as_bytes(), secret2.as_bytes());
    }

    #[test]
    fn session_secret_is_32_bytes() {
        let secret = SessionSecret::random();
        assert_eq!(secret.as_bytes().len(), 32);
    }

    #[test]
    fn server_file_config_parses_a_full_toml_document() {
        let toml = r#"
            bind = "0.0.0.0"
            port = 9000
            corsOrigins = ["https://files.example.com"]
            roots = ["/home/user/documents"]
            maxBodyBytes = 1048576
            maxMutationsPerSecond = 5
            devModeAuthDisabled = false
            tlsCert = "/etc/fm-server/cert.pem"
            tlsKey = "/etc/fm-server/key.pem"
        "#;
        let config: ServerFileConfig = toml::from_str(toml).expect("valid TOML must parse");
        assert_eq!(config.bind, Some(IpAddr::from([0, 0, 0, 0])));
        assert_eq!(config.port, Some(9000));
        assert_eq!(
            config.cors_origins,
            Some(vec!["https://files.example.com".to_owned()])
        );
        assert_eq!(
            config.roots,
            Some(vec![PathBuf::from("/home/user/documents")])
        );
        assert_eq!(config.max_body_bytes, Some(1_048_576));
        assert_eq!(config.max_mutations_per_second, Some(5));
        assert_eq!(config.dev_mode_auth_disabled, Some(false));
        assert_eq!(
            config.tls_cert,
            Some(PathBuf::from("/etc/fm-server/cert.pem"))
        );
        assert_eq!(
            config.tls_key,
            Some(PathBuf::from("/etc/fm-server/key.pem"))
        );
    }

    #[test]
    fn server_file_config_defaults_every_field_to_none_when_empty() {
        let config: ServerFileConfig = toml::from_str("").expect("empty TOML must parse");
        assert!(config.bind.is_none());
        assert!(config.port.is_none());
        assert!(config.cors_origins.is_none());
        assert!(config.roots.is_none());
        assert!(config.tls_cert.is_none());
    }

    #[test]
    fn server_file_config_load_reports_a_typed_error_for_a_missing_file() {
        let error = ServerFileConfig::load(Path::new("/nonexistent/fm-server.toml"))
            .expect_err("missing file must fail");
        assert!(matches!(error, ServerFileConfigError::Read { .. }));
    }

    #[test]
    fn server_file_config_load_reports_a_typed_error_for_invalid_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fm-server.toml");
        std::fs::write(&path, "not valid toml {{{").expect("write fixture");

        let error = ServerFileConfig::load(&path).expect_err("invalid TOML must fail");
        assert!(matches!(error, ServerFileConfigError::Parse { .. }));
    }

    #[test]
    fn server_file_config_load_round_trips_a_real_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fm-server.toml");
        std::fs::write(&path, "port = 9191\n").expect("write fixture");

        let config = ServerFileConfig::load(&path).expect("valid file must load");
        assert_eq!(config.port, Some(9191));
    }
}
