//! Bridges saved FTP profiles and credentials into the FTP provider.
use async_trait::async_trait;
use fm_connections::{
    ConnectionConfiguration, ConnectionDialer, ConnectionError, ConnectionId, ConnectionProfile,
    ConnectionRepository, JsonFileConnectionRepository,
};
use fm_credentials::{CredentialStore, ResolvedCredential, SecretMaterial};
use fm_vfs::VfsError;
use fm_vfs_ftp::{FtpConnectionParameters, FtpConnectionResolver, FtpFileSystemProvider};
use std::{str::FromStr, sync::Arc};
fn parameters(
    profile: &ConnectionProfile,
    credential: Option<&ResolvedCredential>,
) -> Result<FtpConnectionParameters, String> {
    let (c, tls) = match &profile.configuration {
        ConnectionConfiguration::Ftp(v) => (v, false),
        ConnectionConfiguration::Ftps(v) => (v, true),
        _ => return Err("connection is not FTP or FTPS".to_owned()),
    };
    let Some(SecretMaterial::Password { password }) = credential.map(|v| &v.secret) else {
        return Err("FTP authentication requires a stored password credential".to_owned());
    };
    Ok(FtpConnectionParameters {
        host: c.host.clone(),
        port: c.port,
        username: c.username.clone(),
        password: password.to_string(),
        explicit_tls: tls,
    })
}
pub(crate) struct FtpDialer;
#[async_trait]
impl ConnectionDialer for FtpDialer {
    async fn dial(
        &self,
        p: &ConnectionProfile,
        c: Option<&ResolvedCredential>,
    ) -> Result<(), ConnectionError> {
        let v = parameters(p, c).map_err(ConnectionError::DialFailed)?;
        FtpFileSystemProvider::verify_connectivity(&v)
            .await
            .map_err(|e| ConnectionError::DialFailed(e.to_string()))
    }
}
pub(crate) struct FtpResolver {
    repository: JsonFileConnectionRepository,
    credentials: Arc<dyn CredentialStore>,
}
impl FtpResolver {
    pub(crate) fn new(
        repository: JsonFileConnectionRepository,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            repository,
            credentials,
        }
    }
}
#[async_trait]
impl FtpConnectionResolver for FtpResolver {
    async fn resolve(&self, text: &str) -> Result<FtpConnectionParameters, VfsError> {
        let location = format!("ftp://{text}/");
        let id = ConnectionId::from_str(text).map_err(|_| VfsError::InvalidLocation {
            location: location.clone(),
        })?;
        let profile = self
            .repository
            .load(id)
            .await
            .map_err(|e| VfsError::Io {
                message: e.to_string(),
            })?
            .ok_or_else(|| VfsError::NotFound {
                location: location.clone(),
            })?;
        let credential = match &profile.credential_ref {
            Some(r) => Some(
                self.credentials
                    .resolve(r)
                    .await
                    .map_err(|_| VfsError::CredentialRequired)?,
            ),
            None => None,
        };
        parameters(&profile, credential.as_ref()).map_err(|message| VfsError::Io { message })
    }
}
