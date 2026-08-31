//! Shared test-only fixtures for the `onedrive` module (task 0110 focused
//! review). `#[cfg(test)]`-gated from `onedrive/mod.rs`, so it is visible
//! throughout this module's own test code (`token`, `dialer`, `graph`, and
//! this module's own tests) without being part of the crate's production
//! build.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use url::Url;

/// A loopback server that accepts every connection and then never reads or
/// writes anything - simulating a completely unresponsive peer, so a test
/// can prove an HTTP client's *own* configured timeout (not some external
/// one) is what bounds a stalled request, rather than the caller hanging
/// forever (task 0110 review: "ensure token resolver/dialer cannot wait
/// forever").
///
/// Accepted connections are held open (not dropped) for the fixture's
/// lifetime, so the client side gets no FIN/RST to react to - only its own
/// timeout can end the wait.
pub(crate) struct StalledServer {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    _held_connections: Arc<Mutex<Vec<TcpStream>>>,
}

impl Drop for StalledServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl StalledServer {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture must bind a loopback port");
        let addr = listener
            .local_addr()
            .expect("bound listener must report its address");
        let held: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
        let held_for_task = Arc::clone(&held);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                held_for_task.lock().await.push(stream);
            }
        });
        Self {
            addr,
            task,
            _held_connections: held,
        }
    }

    pub(crate) fn base_url(&self) -> Url {
        Url::parse(&format!("http://{}", self.addr)).expect("loopback address always parses")
    }
}
