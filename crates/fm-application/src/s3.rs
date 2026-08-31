//! Bridges saved S3 profiles and credentials into the S3 provider (task 0146).
use async_trait::async_trait;
use fm_connections::{
    ConnectionConfiguration, ConnectionDialer, ConnectionError, ConnectionId, ConnectionProfile,
    ConnectionRepository, JsonFileConnectionRepository,
};
use fm_credentials::{CredentialStore, ResolvedCredential, SecretMaterial};
use fm_vfs::VfsError;
use fm_vfs_s3::{S3ConnectionParameters, S3ConnectionResolver, S3FileSystemProvider};
use std::{str::FromStr, sync::Arc};

fn parameters(
    profile: &ConnectionProfile,
    credential: Option<&ResolvedCredential>,
) -> Result<S3ConnectionParameters, String> {
    let ConnectionConfiguration::S3(config) = &profile.configuration else {
        return Err("connection is not S3".to_owned());
    };
    let Some(SecretMaterial::AccessKey {
        access_key_id,
        secret_access_key,
    }) = credential.map(|value| &value.secret)
    else {
        return Err("S3 authentication requires a stored access key credential".to_owned());
    };
    if *access_key_id != config.access_key_id {
        return Err("stored access key id does not match the connection configuration".to_owned());
    }
    Ok(S3ConnectionParameters {
        endpoint: config.endpoint.clone(),
        region: config
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_owned()),
        bucket: config.bucket.clone(),
        access_key_id: access_key_id.clone(),
        secret_access_key: secret_access_key.to_string(),
    })
}

pub(crate) struct S3Dialer;

#[async_trait]
impl ConnectionDialer for S3Dialer {
    async fn dial(
        &self,
        profile: &ConnectionProfile,
        credential: Option<&ResolvedCredential>,
    ) -> Result<(), ConnectionError> {
        let params = parameters(profile, credential).map_err(ConnectionError::DialFailed)?;
        S3FileSystemProvider::verify_connectivity(&params)
            .await
            .map_err(|error| ConnectionError::DialFailed(error.to_string()))
    }
}

pub(crate) struct S3Resolver {
    repository: JsonFileConnectionRepository,
    credentials: Arc<dyn CredentialStore>,
}

impl S3Resolver {
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
impl S3ConnectionResolver for S3Resolver {
    async fn resolve(&self, connection_id: &str) -> Result<S3ConnectionParameters, VfsError> {
        let location = format!("s3://{connection_id}/");
        let id = ConnectionId::from_str(connection_id).map_err(|_| VfsError::InvalidLocation {
            location: location.clone(),
        })?;
        let profile = self
            .repository
            .load(id)
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?
            .ok_or_else(|| VfsError::NotFound {
                location: location.clone(),
            })?;
        let credential = match &profile.credential_ref {
            Some(reference) => Some(
                self.credentials
                    .resolve(reference)
                    .await
                    .map_err(|_| VfsError::CredentialRequired)?,
            ),
            None => None,
        };
        parameters(&profile, credential.as_ref()).map_err(|message| VfsError::Io { message })
    }
}
