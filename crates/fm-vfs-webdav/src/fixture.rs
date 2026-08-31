//! In-process WebDAV server fixture for tests (task 0147).
//!
//! Mirrors `fm_ssh::fixture`/`fm_vfs_ftp::fixture::FtpFixture`: an isolated,
//! real-protocol server bound to an ephemeral loopback port, exercising the
//! actual WebDAV-over-HTTP wire format (`PROPFIND` `multistatus` XML,
//! `MKCOL`/`PUT`/`GET`/`DELETE`/`MOVE`/`COPY`, `Authorization`/
//! `WWW-Authenticate` for both Basic and Digest) rather than a mocked
//! provider. It is a hand-rolled HTTP/1.1 responder over a raw
//! [`tokio::net::TcpStream`], not built on `hyper`/`axum`: this crate is a
//! layer-2 engine crate (`fm-test-support`'s architecture fitness test
//! reserves `hyper`/`axum` for `apps/` host binaries only), and this
//! fixture's `fixture` module is `pub` (not `#[cfg(test)]`) so
//! `fm-application`'s integration tests can reuse it too, exactly like the
//! two fixtures it mirrors.
//!
//! No external Docker/Nextcloud test container is available in this
//! workspace's sandboxed build environment (no `docker` binary, task 0147's
//! Agent Notes record this as a checked, not assumed, fact) — this in-process
//! server, which speaks the real wire protocol end to end (real XML
//! `multistatus` bodies, real Basic/Digest challenge-response, real TLS
//! certificate rejection when misused), is the closest available substitute
//! and is documented as a known gap rather than silently substituted.
//!
//! Each response includes `Connection: close`; the client is expected to
//! open a fresh TCP connection per request (which `reqwest` does
//! automatically), keeping this fixture's per-connection handling simple.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine;
use md5::{Digest as _, Md5};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// Login name every [`WebDavFixture`] accepts.
pub const FIXTURE_USERNAME: &str = "user";
/// Password every [`WebDavFixture`] accepts.
pub const FIXTURE_PASSWORD: &str = "secret";

const DIGEST_REALM: &str = "fm-fixture";
const DIGEST_NONCE: &str = "fm-fixture-nonce-0001";
const DIGEST_OPAQUE: &str = "fm-fixture-opaque";

/// How a [`WebDavFixture`] challenges requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureAuth {
    /// HTTP Basic authentication.
    Basic,
    /// HTTP Digest authentication (`MD5`, `qop=auth`).
    Digest,
}

#[derive(Clone)]
enum StoredEntry {
    File(Vec<u8>),
    Directory,
}

type Entries = Arc<Mutex<HashMap<String, StoredEntry>>>;
type Locked = Arc<Mutex<std::collections::HashSet<String>>>;

/// A running in-process WebDAV server.
///
/// Dropping the fixture aborts its accept loop and every connection it served.
pub struct WebDavFixture {
    /// Loopback address the fixture is listening on.
    pub addr: SocketAddr,
    /// `http://127.0.0.1:<port>/dav`, ready to use as a connection's
    /// `base_url`.
    pub base_url: String,
    entries: Entries,
    locked: Locked,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for WebDavFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl WebDavFixture {
    /// Starts a plain-HTTP fixture on an ephemeral loopback port with an
    /// empty root, challenging requests with `auth`.
    pub async fn start(auth: FixtureAuth) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the fixture must bind a loopback port");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its address");
        let entries: Entries = Arc::new(Mutex::new(HashMap::new()));
        let locked: Locked = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let served_entries = Arc::clone(&entries);
        let served_locked = Arc::clone(&locked);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let entries = Arc::clone(&served_entries);
                let locked = Arc::clone(&served_locked);
                tokio::spawn(async move { serve(stream, entries, locked, auth).await });
            }
        });
        Self {
            addr,
            base_url: format!("http://{addr}/dav"),
            entries,
            locked,
            task,
        }
    }

    /// Starts a TLS fixture presenting `certificate_der`/`key_der`, for
    /// exercising real certificate validation. Always uses Basic
    /// authentication (TLS itself is what is under test here).
    pub async fn start_tls(certificate_der: Vec<u8>, key_der: Vec<u8>) -> Self {
        use tokio_rustls::TlsAcceptor;
        use tokio_rustls::rustls::ServerConfig;
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certificate = CertificateDer::from(certificate_der);
        let key = PrivateKeyDer::try_from(key_der).expect("valid PKCS8 key");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], key)
            .expect("valid TLS server configuration");
        let acceptor = TlsAcceptor::from(Arc::new(config));

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the fixture must bind a loopback port");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its address");
        let entries: Entries = Arc::new(Mutex::new(HashMap::new()));
        let locked: Locked = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let served_entries = Arc::clone(&entries);
        let served_locked = Arc::clone(&locked);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let entries = Arc::clone(&served_entries);
                let locked = Arc::clone(&served_locked);
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        serve(tls_stream, entries, locked, FixtureAuth::Basic).await;
                    }
                });
            }
        });
        Self {
            addr,
            base_url: format!("https://{addr}/dav"),
            entries,
            locked,
            task,
        }
    }

    /// Seeds a file at an absolute remote path, e.g. `/report.txt`.
    pub async fn put(&self, path: &str, body: &[u8]) {
        self.entries
            .lock()
            .await
            .insert(path.to_owned(), StoredEntry::File(body.to_vec()));
    }

    /// Seeds a directory at an absolute remote path, e.g. `/downloads`.
    pub async fn create_directory(&self, path: &str) {
        self.entries
            .lock()
            .await
            .insert(path.to_owned(), StoredEntry::Directory);
    }

    /// Marks a resource as WebDAV-locked; requests targeting it receive
    /// `423 Locked`.
    pub async fn lock(&self, path: &str) {
        self.locked.lock().await.insert(path.to_owned());
    }

    /// Reads a file back, or `None` when it does not exist.
    pub async fn get(&self, path: &str) -> Option<Vec<u8>> {
        match self.entries.lock().await.get(path) {
            Some(StoredEntry::File(body)) => Some(body.clone()),
            _ => None,
        }
    }

    /// Returns every stored file path, sorted, for leftover-temporary
    /// assertions.
    pub async fn paths(&self) -> Vec<String> {
        let mut paths: Vec<_> = self
            .entries
            .lock()
            .await
            .iter()
            .filter(|(_, entry)| matches!(entry, StoredEntry::File(_)))
            .map(|(path, _)| path.clone())
            .collect();
        paths.sort();
        paths
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_request<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<ParsedRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).await.ok()? == 0 {
        return None;
    }
    let mut parts = request_line.trim_end().splitn(3, ' ');
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        read_chunked_body(reader).await?
    } else {
        let content_length: usize = headers
            .get("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0_u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).await.ok()?;
        }
        body
    };
    Some(ParsedRequest {
        method,
        path,
        headers,
        body,
    })
}

/// Reads an HTTP/1.1 chunked-transfer-encoded body (RFC 9112 §7.1).
/// `reqwest` uses chunked encoding for streaming request bodies of unknown
/// length, which is exactly how this provider's `open_write` uploads.
async fn read_chunked_body<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).await.ok()?;
        let size_text = size_line.trim().split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).ok()?;
        if size == 0 {
            // A trailing header block may follow the zero-size chunk; read
            // until the blank line that terminates it.
            loop {
                let mut trailer = String::new();
                reader.read_line(&mut trailer).await.ok()?;
                if trailer.trim().is_empty() {
                    break;
                }
            }
            break;
        }
        let mut chunk = vec![0_u8; size];
        reader.read_exact(&mut chunk).await.ok()?;
        body.extend_from_slice(&chunk);
        let mut terminator = String::new();
        reader.read_line(&mut terminator).await.ok()?;
    }
    Some(body)
}

fn hex_md5(input: &str) -> String {
    let digest = Md5::digest(input.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn authorized(request: &ParsedRequest, auth: FixtureAuth) -> bool {
    let Some(header) = request.headers.get("authorization") else {
        return false;
    };
    match auth {
        FixtureAuth::Basic => {
            let Some(encoded) = header.strip_prefix("Basic ") else {
                return false;
            };
            let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
                return false;
            };
            let Ok(text) = String::from_utf8(decoded) else {
                return false;
            };
            text == format!("{FIXTURE_USERNAME}:{FIXTURE_PASSWORD}")
        }
        FixtureAuth::Digest => {
            let Some(rest) = header.strip_prefix("Digest ") else {
                return false;
            };
            let params = parse_digest_params(rest);
            let (Some(username), Some(uri), Some(response), Some(nc), Some(cnonce)) = (
                params.get("username"),
                params.get("uri"),
                params.get("response"),
                params.get("nc"),
                params.get("cnonce"),
            ) else {
                return false;
            };
            if username != FIXTURE_USERNAME {
                return false;
            }
            let ha1 = hex_md5(&format!(
                "{FIXTURE_USERNAME}:{DIGEST_REALM}:{FIXTURE_PASSWORD}"
            ));
            let ha2 = hex_md5(&format!("{}:{uri}", request.method));
            let expected = hex_md5(&format!("{ha1}:{DIGEST_NONCE}:{nc}:{cnonce}:auth:{ha2}"));
            &expected == response
        }
    }
}

fn parse_digest_params(rest: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut parts = Vec::new();
    for ch in rest.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            params.insert(
                key.trim().to_ascii_lowercase(),
                value.trim().trim_matches('"').to_owned(),
            );
        }
    }
    params
}

fn challenge_header(auth: FixtureAuth) -> String {
    match auth {
        FixtureAuth::Basic => format!("WWW-Authenticate: Basic realm=\"{DIGEST_REALM}\"\r\n"),
        FixtureAuth::Digest => format!(
            "WWW-Authenticate: Digest realm=\"{DIGEST_REALM}\", nonce=\"{DIGEST_NONCE}\", \
             opaque=\"{DIGEST_OPAQUE}\", qop=\"auth\", algorithm=MD5\r\n"
        ),
    }
}

fn destination_path(request: &ParsedRequest) -> Option<String> {
    let raw = request.headers.get("destination")?;
    let path = raw
        .split_once("/dav")
        .map(|(_, rest)| format!("/dav{rest}"))?;
    Some(strip_dav_prefix(&path))
}

fn strip_dav_prefix(path: &str) -> String {
    path.strip_prefix("/dav").unwrap_or(path).to_owned()
}

fn propfind_response(entries: &HashMap<String, StoredEntry>, request_path: &str) -> String {
    let base = format!("/dav{request_path}");
    let mut body =
        String::from(r#"<?xml version="1.0" encoding="utf-8"?><D:multistatus xmlns:D="DAV:">"#);
    let is_self_dir = matches!(entries.get(request_path), Some(StoredEntry::Directory))
        || request_path.is_empty()
        || request_path == "/";
    if is_self_dir {
        body.push_str(&format!(
            "<D:response><D:href>{base}/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
        ));
    } else if let Some(StoredEntry::File(bytes)) = entries.get(request_path) {
        body.push_str(&format!(
            "<D:response><D:href>{base}</D:href><D:propstat><D:prop><D:resourcetype/><D:getcontentlength>{}</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
            bytes.len()
        ));
    }
    let prefix = if request_path.is_empty() {
        "/".to_owned()
    } else {
        format!("{request_path}/")
    };
    for (path, entry) in entries {
        let Some(rest) = path.strip_prefix(&prefix) else {
            continue;
        };
        if rest.is_empty() || rest.contains('/') {
            continue;
        }
        let child_base = format!("/dav{path}");
        match entry {
            StoredEntry::Directory => body.push_str(&format!(
                "<D:response><D:href>{child_base}/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>"
            )),
            StoredEntry::File(bytes) => body.push_str(&format!(
                "<D:response><D:href>{child_base}</D:href><D:propstat><D:prop><D:resourcetype/><D:getcontentlength>{}</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>",
                bytes.len()
            )),
        }
    }
    body.push_str("</D:multistatus>");
    body
}

async fn write_response<W: AsyncWriteExt + Unpin>(
    write: &mut W,
    status: &str,
    extra_headers: &str,
    body: &[u8],
) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n",
        body.len()
    );
    let _ = write.write_all(head.as_bytes()).await;
    let _ = write.write_all(body).await;
    let _ = write.shutdown().await;
}

async fn serve<S>(stream: S, entries: Entries, locked: Locked, auth: FixtureAuth)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let Some(request) = read_request(&mut reader).await else {
        return;
    };

    if !authorized(&request, auth) {
        write_response(
            &mut write_half,
            "401 Unauthorized",
            &challenge_header(auth),
            b"",
        )
        .await;
        return;
    }

    let path = strip_dav_prefix(&request.path);

    if locked.lock().await.contains(&path) && request.method != "PROPFIND" {
        write_response(&mut write_half, "423 Locked", "", b"").await;
        return;
    }

    match request.method.as_str() {
        "HEAD" => {
            write_response(&mut write_half, "200 OK", "Accept-Ranges: bytes\r\n", b"").await;
        }
        "OPTIONS" => {
            write_response(&mut write_half, "200 OK", "DAV: 1\r\n", b"").await;
        }
        "PROPFIND" => {
            let entries_guard = entries.lock().await;
            let exists = path.is_empty()
                || path == "/"
                || entries_guard.contains_key(&path)
                || entries_guard
                    .keys()
                    .any(|candidate| candidate.starts_with(&format!("{path}/")));
            if !exists {
                drop(entries_guard);
                write_response(&mut write_half, "404 Not Found", "", b"").await;
                return;
            }
            let body = propfind_response(&entries_guard, &path);
            drop(entries_guard);
            write_response(
                &mut write_half,
                "207 Multi-Status",
                "Content-Type: application/xml; charset=utf-8\r\n",
                body.as_bytes(),
            )
            .await;
        }
        "GET" => {
            let body = entries
                .lock()
                .await
                .get(&path)
                .and_then(|entry| match entry {
                    StoredEntry::File(bytes) => Some(bytes.clone()),
                    StoredEntry::Directory => None,
                });
            match body {
                Some(bytes) => write_response(&mut write_half, "200 OK", "", &bytes).await,
                None => write_response(&mut write_half, "404 Not Found", "", b"").await,
            }
        }
        "PUT" => {
            entries
                .lock()
                .await
                .insert(path, StoredEntry::File(request.body));
            write_response(&mut write_half, "201 Created", "", b"").await;
        }
        "DELETE" => {
            let removed = entries.lock().await.remove(&path).is_some();
            if removed {
                write_response(&mut write_half, "204 No Content", "", b"").await;
            } else {
                write_response(&mut write_half, "404 Not Found", "", b"").await;
            }
        }
        "MKCOL" => {
            entries.lock().await.insert(path, StoredEntry::Directory);
            write_response(&mut write_half, "201 Created", "", b"").await;
        }
        "MOVE" => {
            let Some(destination) = destination_path(&request) else {
                write_response(&mut write_half, "400 Bad Request", "", b"").await;
                return;
            };
            let mut entries_guard = entries.lock().await;
            match entries_guard.remove(&path) {
                Some(value) => {
                    entries_guard.insert(destination, value);
                    drop(entries_guard);
                    write_response(&mut write_half, "201 Created", "", b"").await;
                }
                None => {
                    drop(entries_guard);
                    write_response(&mut write_half, "404 Not Found", "", b"").await;
                }
            }
        }
        "COPY" => {
            let Some(destination) = destination_path(&request) else {
                write_response(&mut write_half, "400 Bad Request", "", b"").await;
                return;
            };
            let mut entries_guard = entries.lock().await;
            match entries_guard.get(&path).cloned() {
                Some(value) => {
                    entries_guard.insert(destination, value);
                    drop(entries_guard);
                    write_response(&mut write_half, "201 Created", "", b"").await;
                }
                None => {
                    drop(entries_guard);
                    write_response(&mut write_half, "404 Not Found", "", b"").await;
                }
            }
        }
        _ => {
            write_response(&mut write_half, "501 Not Implemented", "", b"").await;
        }
    }
}
