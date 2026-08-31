//! Bridges saved WebDAV profiles and credentials into the WebDAV provider.
use async_trait::async_trait;
use fm_connections::{
    ConnectionConfiguration, ConnectionDialer, ConnectionError, ConnectionId, ConnectionProfile,
    ConnectionRepository, JsonFileConnectionRepository, WebDavAuthenticationScheme,
};
use fm_credentials::{CredentialStore, ResolvedCredential, SecretMaterial};
use fm_vfs::VfsError;
use fm_vfs_webdav::{
    WebDavAuthScheme, WebDavConnectionParameters, WebDavConnectionResolver,
    WebDavFileSystemProvider,
};
use std::{str::FromStr, sync::Arc};

fn parameters(
    profile: &ConnectionProfile,
    credential: Option<&ResolvedCredential>,
) -> Result<WebDavConnectionParameters, String> {
    let ConnectionConfiguration::WebDav(config) = &profile.configuration else {
        return Err("connection is not WebDAV".to_owned());
    };
    let Some(SecretMaterial::Password { password }) = credential.map(|v| &v.secret) else {
        return Err("WebDAV authentication requires a stored password credential".to_owned());
    };
    Ok(WebDavConnectionParameters {
        base_url: config.base_url.clone(),
        username: config.username.clone(),
        password: password.to_string(),
        auth_scheme: match config.authentication {
            WebDavAuthenticationScheme::Basic => WebDavAuthScheme::Basic,
            WebDavAuthenticationScheme::Digest => WebDavAuthScheme::Digest,
        },
    })
}

pub(crate) struct WebDavDialer;
#[async_trait]
impl ConnectionDialer for WebDavDialer {
    async fn dial(
        &self,
        p: &ConnectionProfile,
        c: Option<&ResolvedCredential>,
    ) -> Result<(), ConnectionError> {
        let v = parameters(p, c).map_err(ConnectionError::DialFailed)?;
        WebDavFileSystemProvider::verify_connectivity(&v)
            .await
            .map_err(|e| ConnectionError::DialFailed(e.to_string()))
    }
}

pub(crate) struct WebDavResolver {
    repository: JsonFileConnectionRepository,
    credentials: Arc<dyn CredentialStore>,
}
impl WebDavResolver {
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
impl WebDavConnectionResolver for WebDavResolver {
    async fn resolve(&self, text: &str) -> Result<WebDavConnectionParameters, VfsError> {
        let location = format!("webdav://{text}/");
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
