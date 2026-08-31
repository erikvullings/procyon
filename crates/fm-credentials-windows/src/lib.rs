//! Windows Credential Manager implementation of
//! [`fm_credentials::CredentialStore`] (task 0103).
//!
//! The crate is a workspace member on every OS but compiles to nothing off
//! Windows (see `docs/decisions/0010-native-platform-adapters.md`, mirroring
//! `fm-platform-macos`/`fm-platform-windows`). Its raw `Cred*` FFI calls are
//! inherently `unsafe`, so this crate is isolated specifically to keep the
//! rest of the workspace's `unsafe_code = "deny"` lint intact.
//!
//! Not runtime-tested: this development machine has no Windows target
//! available, so only the platform-neutral logic (target-name formatting,
//! error mapping) is covered by tests here; the `Cred*` calls themselves
//! could only be exercised on a real Windows host or CI runner.

#![cfg(target_os = "windows")]
#![allow(unsafe_code)]

use std::ptr;

use async_trait::async_trait;
use fm_credentials::{
    CredentialError, CredentialRef, CredentialStore, ResolvedCredential, StoreCredentialRequest,
    codec,
};
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError};
use windows_sys::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};

/// Prefix every credential's Credential Manager target name is built from.
/// The suffix is the credential's [`CredentialRef`] (a random UUID), so
/// entries from different connections never collide.
const TARGET_PREFIX: &str = "dev.fm.credentials/";

fn target_name(reference: &CredentialRef) -> String {
    format!("{TARGET_PREFIX}{reference}")
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn map_last_error(reference: CredentialRef) -> CredentialError {
    let code = unsafe { GetLastError() };
    if code == ERROR_NOT_FOUND {
        CredentialError::NotFound { reference }
    } else {
        CredentialError::Backend(format!("windows credential manager error {code}"))
    }
}

/// [`CredentialStore`] backed by Windows Credential Manager generic
/// credentials (spec §5.3, §19; task 0103's acceptance criterion "Windows
/// uses Credential Manager or equivalent protected storage").
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsCredentialStore;

impl WindowsCredentialStore {
    /// Creates a store backed by the current user's Credential Manager
    /// vault.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

async fn run_blocking<T, F>(f: F) -> Result<T, CredentialError>
where
    F: FnOnce() -> Result<T, CredentialError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f).await.unwrap_or_else(|_| {
        Err(CredentialError::Backend(
            "credential manager task panicked".to_owned(),
        ))
    })
}

fn write_credential(reference: CredentialRef, mut bytes: Vec<u8>) -> Result<(), CredentialError> {
    let mut target_wide = to_wide(&target_name(&reference));
    let blob_size = u32::try_from(bytes.len())
        .map_err(|_| CredentialError::Backend("secret too large".to_owned()))?;

    let credential = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target_wide.as_mut_ptr(),
        Comment: ptr::null_mut(),
        LastWritten: Default::default(),
        CredentialBlobSize: blob_size,
        CredentialBlob: bytes.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: ptr::null_mut(),
        TargetAlias: ptr::null_mut(),
        UserName: ptr::null_mut(),
    };

    let succeeded = unsafe { CredWriteW(&credential, 0) };
    if succeeded == 0 {
        Err(map_last_error(reference))
    } else {
        Ok(())
    }
}

fn read_credential(reference: CredentialRef) -> Result<Vec<u8>, CredentialError> {
    let target_wide = to_wide(&target_name(&reference));
    let mut credential_ptr: *mut CREDENTIALW = ptr::null_mut();

    let succeeded = unsafe {
        CredReadW(
            target_wide.as_ptr(),
            CRED_TYPE_GENERIC,
            0,
            &mut credential_ptr,
        )
    };
    if succeeded == 0 {
        return Err(map_last_error(reference));
    }

    // Safety: `CredReadW` reported success, so `credential_ptr` points at a
    // valid `CREDENTIALW` that Windows owns until `CredFree` is called.
    let bytes = unsafe {
        let credential = &*credential_ptr;
        std::slice::from_raw_parts(
            credential.CredentialBlob,
            credential.CredentialBlobSize as usize,
        )
        .to_vec()
    };
    unsafe { CredFree(credential_ptr.cast()) };
    Ok(bytes)
}

fn delete_credential(reference: CredentialRef) -> Result<(), CredentialError> {
    let target_wide = to_wide(&target_name(&reference));
    let succeeded = unsafe { CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0) };
    if succeeded == 0 {
        Err(map_last_error(reference))
    } else {
        Ok(())
    }
}

#[async_trait]
impl CredentialStore for WindowsCredentialStore {
    async fn store(
        &self,
        request: StoreCredentialRequest,
    ) -> Result<CredentialRef, CredentialError> {
        let reference = CredentialRef::new();
        let bytes = codec::encode(&request.secret);
        run_blocking(move || write_credential(reference, bytes)).await?;
        Ok(reference)
    }

    async fn resolve(
        &self,
        reference: &CredentialRef,
    ) -> Result<ResolvedCredential, CredentialError> {
        let reference = *reference;
        let bytes = run_blocking(move || read_credential(reference)).await?;
        let secret = codec::decode(&bytes)?;
        Ok(ResolvedCredential { secret })
    }

    async fn delete(&self, reference: &CredentialRef) -> Result<(), CredentialError> {
        let reference = *reference;
        run_blocking(move || delete_credential(reference)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_name_embeds_the_credential_reference() {
        let reference = CredentialRef::new();
        assert_eq!(
            target_name(&reference),
            format!("{TARGET_PREFIX}{reference}")
        );
    }

    #[test]
    fn to_wide_is_null_terminated() {
        let wide = to_wide("abc");
        assert_eq!(wide, vec![b'a' as u16, b'b' as u16, b'c' as u16, 0]);
    }
}
