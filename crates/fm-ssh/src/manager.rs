//! [`SshConnectionManager`]: pools live [`SshSession`]s and reconnects them
//! transparently (task 0104, spec §6.2, §6.8 "reconnect for browsing").

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::SshError;
use crate::known_hosts::{HostKeyVerification, KnownHostsStore};
use crate::session::SshSession;
use crate::types::{SshConnectTarget, SshConnectionParameters};

/// Owns zero or more live [`SshSession`]s, keyed by an opaque caller-chosen
/// string (task 0104: a connection id's text form), and the shared
/// [`KnownHostsStore`] every session verifies its host key against.
///
/// A session is reused across calls for the same key; if it has died (the
/// underlying transport closed), the next [`Self::session`] call
/// transparently redials rather than surfacing a stale-session error to the
/// caller - satisfying spec §6.8's "reconnect for browsing" without
/// requiring an explicit user-initiated reconnect for read-only work.
pub struct SshConnectionManager {
    known_hosts: Arc<dyn KnownHostsStore>,
    sessions: Mutex<HashMap<String, Arc<SshSession>>>,
}

impl SshConnectionManager {
    /// Creates a manager backed by `known_hosts`.
    #[must_use]
    pub fn new(known_hosts: Arc<dyn KnownHostsStore>) -> Self {
        Self {
            known_hosts,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// The known-hosts store this manager verifies every session against.
    #[must_use]
    pub fn known_hosts(&self) -> &Arc<dyn KnownHostsStore> {
        &self.known_hosts
    }

    /// Returns a live, authenticated session for `key`, connecting (or
    /// reconnecting, if the cached session has died) as needed.
    pub async fn session(
        &self,
        key: &str,
        params: &SshConnectionParameters,
    ) -> Result<Arc<SshSession>, SshError> {
        {
            let sessions = self.sessions.lock().await;
            if let Some(existing) = sessions.get(key)
                && !existing.is_closed()
            {
                return Ok(existing.clone());
            }
        }
        let session = Arc::new(SshSession::connect(params, self.known_hosts.clone(), key).await?);
        self.sessions
            .lock()
            .await
            .insert(key.to_owned(), session.clone());
        Ok(session)
    }

    /// Drops any cached session for `key`. The next [`Self::session`] call
    /// reconnects from scratch. Callers use this after observing an
    /// operation fail in a way that suggests the transport (not just one
    /// request) is dead.
    pub async fn invalidate(&self, key: &str) {
        self.sessions.lock().await.remove(key);
    }

    /// Attempts a one-shot connect and authenticate without caching the
    /// resulting session, for callers that only need to prove connectivity
    /// (the `ConnectionDialer` "connect"/"test" flow).
    pub async fn verify_connectivity(
        &self,
        key: &str,
        params: &SshConnectionParameters,
    ) -> Result<(), SshError> {
        SshSession::connect(params, self.known_hosts.clone(), key)
            .await
            .map(|_| ())
    }

    /// Probes `target`'s host key under `key` without authenticating (spec
    /// §6.4's explicit host-key confirmation flow) - see
    /// [`SshSession::probe_host_key`].
    pub async fn probe_host_key(
        &self,
        key: &str,
        target: &SshConnectTarget,
    ) -> Result<HostKeyVerification, SshError> {
        SshSession::probe_host_key(target, self.known_hosts.clone(), key).await
    }
}
