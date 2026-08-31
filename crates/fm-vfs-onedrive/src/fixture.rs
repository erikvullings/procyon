//! In-process Microsoft Graph fixture for tests (task 0110), following the
//! same "real HTTP/1.1 wire protocol, ephemeral loopback port, no docker,
//! no real Microsoft calls" convention as `fm_vfs_s3::fixture`/
//! `fm_vfs_webdav::fixture`.
//!
//! Two independent loopback listeners are started, mirroring the real
//! Microsoft Graph topology exactly rather than approximating it on one
//! socket:
//!
//! * The **Graph API** listener (`graph_base_url`) serves every
//!   authenticated call: listing, metadata, `createUploadSession`, simple
//!   upload, rename/move, remove, delta.
//! * The **transfer** listener serves preauthenticated download URLs
//!   (`@microsoft.graph.downloadUrl`) and upload-session chunk `PUT`/
//!   `DELETE`s. Using a genuinely different host - not just a different
//!   path - is what lets a test prove a bearer token was never sent there,
//!   rather than merely that a particular header was cleared on the same
//!   client.
//!
//! Every request received by either listener is captured (method, path,
//! headers, body) and exposed through [`GraphFixture::requests`]/
//! [`GraphFixture::transfer_requests`] for exactly this kind of assertion.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// One request captured by either listener, for test assertions.
#[derive(Debug, Clone)]
pub struct CapturedRequest {
    /// HTTP method, e.g. `"GET"`.
    pub method: String,
    /// Request-target path and query string, decoded from the request line.
    pub path: String,
    /// Header names are lower-cased; values are as sent.
    pub headers: HashMap<String, String>,
    /// Raw request body.
    pub body: Vec<u8>,
}

impl CapturedRequest {
    /// Whether an `Authorization` header was present at all - the exact
    /// assertion task 0110 requires at the download/upload transfer target.
    #[must_use]
    pub fn has_authorization_header(&self) -> bool {
        self.headers.contains_key("authorization")
    }
}

#[derive(Clone)]
struct Item {
    id: String,
    /// Decoded, slash-joined path relative to the drive root; empty for the
    /// root itself.
    path_key: String,
    name: String,
    parent_key: Option<String>,
    is_folder: bool,
    content: Vec<u8>,
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    /// Change-log version this item was last touched at (task 0110's delta
    /// tests key on this).
    version: u64,
    deleted: bool,
}

struct PendingUpload {
    path_key: String,
    received: Vec<u8>,
}

#[derive(Default)]
struct Store {
    items: HashMap<String, Item>,
    /// Explicit child ordering override per folder path key, for tests that
    /// need deliberately unsorted/interleaved pages.
    child_order: HashMap<String, Vec<String>>,
    /// Append-only log of path keys touched, in order - backs `/delta`.
    change_log: Vec<String>,
    version: u64,
    next_id: u64,
    uploads: HashMap<String, PendingUpload>,
    next_upload_id: u64,
    /// Statuses to return for the next N Graph-host requests, consumed FIFO
    /// (task 0110's throttling tests).
    throttle_queue: std::collections::VecDeque<(u16, Option<u64>)>,
    /// Delta cursor values that must respond `410 Gone` once (task 0110's
    /// `ResetRequired` test), consumed on first use.
    expired_delta_cursors: std::collections::HashSet<String>,
    /// When `true`, the *next* cursor-bearing delta request receives
    /// `410 Gone` regardless of its specific cursor value, then this
    /// resets to `false` - lets a test force a reset deterministically
    /// without racing to learn an opaque cursor's exact text first.
    expire_next_delta_poll: bool,
}

impl Store {
    fn new() -> Self {
        let mut store = Self::default();
        store.items.insert(
            String::new(),
            Item {
                id: "graph-root-item".to_owned(),
                path_key: String::new(),
                name: String::new(),
                parent_key: None,
                is_folder: true,
                content: Vec::new(),
                created: Utc::now(),
                modified: Utc::now(),
                version: 0,
                deleted: false,
            },
        );
        store
    }

    fn next_item_id(&mut self) -> String {
        self.next_id += 1;
        // Deliberately not UUID-shaped (task 0110: "never assume Graph item
        // IDs are UUIDs") - real Graph ids look like this: a long
        // base64url-ish opaque token, sometimes with an embedded `!`.
        format!("01FIXTUREITEM{:08}!{}", self.next_id, self.next_id * 7 + 3)
    }

    fn bump_version(&mut self) -> u64 {
        self.version += 1;
        self.version
    }

    fn child_keys(&self, parent_key: &str) -> Vec<String> {
        if let Some(order) = self.child_order.get(parent_key) {
            return order.clone();
        }
        let mut keys: Vec<String> = self
            .items
            .values()
            .filter(|item| !item.deleted && item.parent_key.as_deref() == Some(parent_key))
            .map(|item| item.path_key.clone())
            .collect();
        keys.sort();
        keys
    }
}

/// A running in-process Microsoft Graph fixture.
pub struct GraphFixture {
    graph_addr: SocketAddr,
    store: Arc<Mutex<Store>>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    transfer_requests: Arc<Mutex<Vec<CapturedRequest>>>,
    graph_task: tokio::task::JoinHandle<()>,
    transfer_task: tokio::task::JoinHandle<()>,
}

impl Drop for GraphFixture {
    fn drop(&mut self) {
        self.graph_task.abort();
        self.transfer_task.abort();
    }
}

impl GraphFixture {
    /// Starts a fixture on two ephemeral loopback ports with an empty
    /// drive (just the root folder).
    pub async fn start() -> Self {
        let store = Arc::new(Mutex::new(Store::new()));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transfer_requests = Arc::new(Mutex::new(Vec::new()));

        let graph_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding an ephemeral loopback port must succeed");
        let graph_addr = graph_listener
            .local_addr()
            .expect("bound listener has an address");
        let transfer_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding an ephemeral loopback port must succeed");
        let transfer_addr = transfer_listener
            .local_addr()
            .expect("bound listener has an address");

        let graph_store = Arc::clone(&store);
        let graph_requests = Arc::clone(&requests);
        let addrs = Addrs {
            graph: graph_addr,
            transfer: transfer_addr,
        };
        let graph_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = graph_listener.accept().await else {
                    continue;
                };
                let store = Arc::clone(&graph_store);
                let requests = Arc::clone(&graph_requests);
                tokio::spawn(async move {
                    let _ = serve_graph(stream, store, requests, addrs).await;
                });
            }
        });

        let transfer_store = Arc::clone(&store);
        let transfer_captured = Arc::clone(&transfer_requests);
        let transfer_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = transfer_listener.accept().await else {
                    continue;
                };
                let store = Arc::clone(&transfer_store);
                let requests = Arc::clone(&transfer_captured);
                tokio::spawn(async move {
                    let _ = serve_transfer(stream, store, requests).await;
                });
            }
        });

        Self {
            graph_addr,
            store,
            requests,
            transfer_requests,
            graph_task,
            transfer_task,
        }
    }

    /// The Graph API base URL, suitable for [`crate::GraphConfig::new`].
    #[must_use]
    pub fn graph_base_url(&self) -> String {
        format!("http://{}/v1.0", self.graph_addr)
    }

    /// Every request the Graph API listener has received so far, in order.
    pub async fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().await.clone()
    }

    /// Every request the transfer (preauthenticated) listener has received
    /// so far, in order.
    pub async fn transfer_requests(&self) -> Vec<CapturedRequest> {
        self.transfer_requests.lock().await.clone()
    }

    /// Clears captured request history on both listeners, for tests that
    /// seed data before the behaviour under test.
    pub async fn clear_requests(&self) {
        self.requests.lock().await.clear();
        self.transfer_requests.lock().await.clear();
    }

    /// Seeds a folder at `parent_path` (empty for the drive root). Returns
    /// its Graph item id.
    pub async fn create_folder(&self, parent_path: &str, name: &str) -> String {
        let mut store = self.store.lock().await;
        let path_key = join_path(parent_path, name);
        let id = store.next_item_id();
        let version = store.bump_version();
        store.items.insert(
            path_key.clone(),
            Item {
                id: id.clone(),
                path_key: path_key.clone(),
                name: name.to_owned(),
                parent_key: Some(parent_path.to_owned()),
                is_folder: true,
                content: Vec::new(),
                created: Utc::now(),
                modified: Utc::now(),
                version,
                deleted: false,
            },
        );
        store.change_log.push(path_key);
        id
    }

    /// Seeds a file at `parent_path` (empty for the drive root) with
    /// `content`. Returns its Graph item id.
    pub async fn create_file(&self, parent_path: &str, name: &str, content: &[u8]) -> String {
        let mut store = self.store.lock().await;
        let path_key = join_path(parent_path, name);
        let id = store.next_item_id();
        let version = store.bump_version();
        store.items.insert(
            path_key.clone(),
            Item {
                id: id.clone(),
                path_key: path_key.clone(),
                name: name.to_owned(),
                parent_key: Some(parent_path.to_owned()),
                is_folder: false,
                content: content.to_vec(),
                created: Utc::now(),
                modified: Utc::now(),
                version,
                deleted: false,
            },
        );
        store.change_log.push(path_key);
        id
    }

    /// Overrides child ordering for `folder_path` (empty for the root) to
    /// exactly `names`, in this exact order, regardless of insertion order -
    /// used to construct deliberately unsorted/interleaved listing pages.
    pub async fn set_children_order(&self, folder_path: &str, names: Vec<&str>) {
        let mut store = self.store.lock().await;
        let keys = names
            .into_iter()
            .map(|name| join_path(folder_path, name))
            .collect();
        store.child_order.insert(folder_path.to_owned(), keys);
    }

    /// Current stored bytes for a file path, or `None` if absent/deleted.
    pub async fn file_content(&self, path: &str) -> Option<Vec<u8>> {
        let store = self.store.lock().await;
        store
            .items
            .get(path)
            .filter(|item| !item.deleted && !item.is_folder)
            .map(|item| item.content.clone())
    }

    /// Whether a non-deleted item exists at `path`.
    pub async fn exists(&self, path: &str) -> bool {
        let store = self.store.lock().await;
        store.items.get(path).is_some_and(|item| !item.deleted)
    }

    /// Queues a throttled response for the next Graph-host request
    /// (task 0110's `Retry-After`/backoff tests). `retry_after_seconds`
    /// omitted simulates a throttling response with no `Retry-After`
    /// header, exercising the fallback backoff path.
    pub async fn queue_throttle(&self, status: u16, retry_after_seconds: Option<u64>) {
        self.store
            .lock()
            .await
            .throttle_queue
            .push_back((status, retry_after_seconds));
    }

    /// Marks a delta cursor value as expired: the *next* request using it
    /// receives `410 Gone` once, then reverts to responding normally to a
    /// fresh `token=latest` reseed (task 0110's `ResetRequired` handling).
    pub async fn expire_delta_cursor(&self, cursor: &str) {
        self.store
            .lock()
            .await
            .expired_delta_cursors
            .insert(cursor.to_owned());
    }

    /// Forces the *next* cursor-bearing delta request - whatever its
    /// specific cursor value turns out to be - to receive `410 Gone` once.
    /// Race-free alternative to [`Self::expire_delta_cursor`] for a test
    /// that cannot observe an opaque cursor's exact text ahead of time.
    pub async fn expire_next_delta_poll(&self) {
        self.store.lock().await.expire_next_delta_poll = true;
    }
}

fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

struct ParsedRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<ParsedRequest>> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        if let Some(position) = find_subslice(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > 16 * 1024 * 1024 {
            return Ok(None);
        }
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }

    let content_length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(Some(ParsedRequest {
        method,
        path,
        headers,
        body,
    }))
}

struct FixtureResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl FixtureResponse {
    fn json(status: u16, body: serde_json::Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
            body: body.to_string().into_bytes(),
        }
    }

    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn bytes(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            body,
        }
    }
}

async fn write_response(stream: &mut TcpStream, response: &FixtureResponse) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\nContent-Length: {}\r\n",
        response.status,
        reason_phrase(response.status),
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.flush().await
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        412 => "Precondition Failed",
        423 => "Locked",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        507 => "Insufficient Storage",
        _ => "Internal Server Error",
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_path_and_query(raw: &str) -> (String, HashMap<String, String>) {
    let (path, query_string) = raw.split_once('?').unwrap_or((raw, ""));
    let mut query = HashMap::new();
    for pair in query_string.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(percent_decode(key), percent_decode(value));
    }
    (path.to_owned(), query)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|value| u8::from_str_radix(value, 16).ok()) {
                decoded.push(value);
                index += 3;
                continue;
            }
        } else if bytes[index] == b'+' {
            decoded.push(b' ');
            index += 1;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Extracts the item-address path key from a Graph request path such as
/// `/v1.0/me/drive/root:/Documents/My%20Report.pdf:/content` (returns
/// `Some(("Documents/My Report.pdf", "/content"))`) or
/// `/v1.0/me/drive/root/children` (returns `Some(("", "/children"))`).
/// Returns `None` for paths this fixture does not recognize at all.
fn extract_item_address(path: &str) -> Option<(String, String)> {
    let remainder = path.strip_prefix("/v1.0/me/drive/")?;
    if let Some(rest) = remainder.strip_prefix("root:/") {
        // Structural colons never appear inside an encoded path segment
        // (fm-domain never emits a raw `:` from a name containing one -
        // see `fm_vfs_onedrive::graph`'s module doc): the *last* `:` in the
        // remainder is always the closing delimiter.
        let close = rest.rfind(':')?;
        let encoded_path = &rest[..close];
        let suffix = &rest[close + 1..];
        Some((percent_decode(encoded_path), suffix.to_owned()))
    } else {
        remainder
            .strip_prefix("root")
            .map(|rest| (String::new(), rest.to_owned()))
    }
}

/// Extracts a Graph item id from `/v1.0/me/drive/items/{id}` or
/// `/v1.0/me/drive/items/{id}/delta`.
fn extract_item_id(path: &str) -> Option<(String, String)> {
    let remainder = path.strip_prefix("/v1.0/me/drive/items/")?;
    match remainder.split_once('/') {
        Some((id, rest)) => Some((percent_decode(id), format!("/{rest}"))),
        None => Some((percent_decode(remainder), String::new())),
    }
}

fn item_json(item: &Item, transfer_addr: SocketAddr) -> serde_json::Value {
    let mut value = serde_json::json!({
        "id": item.id,
        "name": item.name,
        "lastModifiedDateTime": item.modified.to_rfc3339(),
        "createdDateTime": item.created.to_rfc3339(),
    });
    if item.is_folder {
        value["folder"] = serde_json::json!({ "childCount": 0 });
    } else {
        value["size"] = serde_json::json!(item.content.len());
        value["file"] = serde_json::json!({ "mimeType": "application/octet-stream" });
        value["@microsoft.graph.downloadUrl"] = serde_json::json!(format!(
            "http://{transfer_addr}/download/{}",
            percent_encode(&item.id)
        ));
    }
    value
}

/// The two loopback addresses a response occasionally needs to embed
/// (`downloadUrl`/`uploadUrl` point at `transfer`; opaque continuation
/// links point back at `graph`) - bundled to keep handler signatures within
/// clippy's argument-count lint.
#[derive(Debug, Clone, Copy)]
struct Addrs {
    graph: SocketAddr,
    transfer: SocketAddr,
}

async fn serve_graph(
    mut stream: TcpStream,
    store: Arc<Mutex<Store>>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    addrs: Addrs,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await? else {
        return Ok(());
    };
    let (path_only, query) = parse_path_and_query(&request.path);
    requests.lock().await.push(CapturedRequest {
        method: request.method.clone(),
        path: request.path.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
    });

    if !request.headers.contains_key("authorization") {
        return write_response(&mut stream, &FixtureResponse::empty(401)).await;
    }

    let mut store_guard = store.lock().await;
    if let Some((status, retry_after)) = store_guard.throttle_queue.pop_front() {
        drop(store_guard);
        let mut headers = Vec::new();
        if let Some(seconds) = retry_after {
            headers.push(("Retry-After".to_owned(), seconds.to_string()));
        }
        let response = FixtureResponse {
            status,
            headers,
            body: Vec::new(),
        };
        return write_response(&mut stream, &response).await;
    }

    let response = route_graph(
        &request.method,
        &path_only,
        &query,
        &request.body,
        &mut store_guard,
        addrs,
    );
    drop(store_guard);
    write_response(&mut stream, &response).await
}

fn route_graph(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: &[u8],
    store: &mut Store,
    addrs: Addrs,
) -> FixtureResponse {
    if let Some((item_id, suffix)) = extract_item_id(path) {
        return route_by_item_id(method, &item_id, &suffix, query, body, store, addrs);
    }
    let Some((path_key, suffix)) = extract_item_address(path) else {
        return FixtureResponse::empty(400);
    };

    match (method, suffix.as_str()) {
        ("GET", "") => match store.items.get(&path_key) {
            Some(item) if !item.deleted => {
                FixtureResponse::json(200, item_json(item, addrs.transfer))
            }
            _ => FixtureResponse::json(404, not_found_body()),
        },
        ("GET", "/children") => {
            let Some(parent) = store.items.get(&path_key).filter(|item| !item.deleted) else {
                return FixtureResponse::json(404, not_found_body());
            };
            if !parent.is_folder {
                return FixtureResponse::json(400, not_found_body());
            }
            let top: usize = query
                .get("$top")
                .and_then(|value| value.parse().ok())
                .unwrap_or(200);
            let start: usize = query
                .get("$skiptoken")
                .and_then(|token| token.strip_prefix("cursor_"))
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let keys = store.child_keys(&path_key);
            children_page_response(store, &keys, start, top, addrs, &path_key)
        }
        ("POST", "/children") => create_child(store, &path_key, body, addrs.transfer),
        ("PUT", "/content") => put_content(store, &path_key, body, addrs.transfer),
        ("POST", "/createUploadSession") => create_upload_session(store, &path_key, addrs.transfer),
        ("GET", "/delta") => delta_response(store, &path_key, query, addrs),
        _ => FixtureResponse::empty(400),
    }
}

fn route_by_item_id(
    method: &str,
    item_id: &str,
    suffix: &str,
    query: &HashMap<String, String>,
    body: &[u8],
    store: &mut Store,
    addrs: Addrs,
) -> FixtureResponse {
    match (method, suffix) {
        ("PATCH", "") => patch_item(store, item_id, body, addrs.transfer),
        ("DELETE", "") => delete_item(store, item_id),
        ("GET", "/delta") => {
            let Some(path_key) = store
                .items
                .values()
                .find(|item| item.id == item_id)
                .map(|item| item.path_key.clone())
            else {
                return FixtureResponse::json(404, not_found_body());
            };
            delta_response(store, &path_key, query, addrs)
        }
        _ => FixtureResponse::empty(400),
    }
}

fn not_found_body() -> serde_json::Value {
    serde_json::json!({ "error": { "code": "itemNotFound" } })
}

fn children_page_response(
    store: &Store,
    keys: &[String],
    start: usize,
    top: usize,
    addrs: Addrs,
    parent_key: &str,
) -> FixtureResponse {
    let end = (start + top.max(1)).min(keys.len());
    let page_keys = &keys[start.min(keys.len())..end];
    let value: Vec<serde_json::Value> = page_keys
        .iter()
        .filter_map(|key| store.items.get(key))
        .map(|item| item_json(item, addrs.transfer))
        .collect();
    let mut body = serde_json::json!({ "value": value });
    if end < keys.len() {
        let cursor = format!("cursor_{end}");
        let encoded_parent = percent_encode(parent_key);
        let graph_addr = addrs.graph;
        let next_link = if parent_key.is_empty() {
            format!(
                "http://{graph_addr}/v1.0/me/drive/root/children?$top={top}&$skiptoken={cursor}"
            )
        } else {
            format!(
                "http://{graph_addr}/v1.0/me/drive/root:/{encoded_parent}:/children?$top={top}&$skiptoken={cursor}"
            )
        };
        body["@odata.nextLink"] = serde_json::json!(next_link);
    }
    FixtureResponse::json(200, body)
}

fn create_child(
    store: &mut Store,
    parent_key: &str,
    body: &[u8],
    transfer_addr: SocketAddr,
) -> FixtureResponse {
    let Some(parent) = store.items.get(parent_key).filter(|item| !item.deleted) else {
        return FixtureResponse::json(404, not_found_body());
    };
    if !parent.is_folder {
        return FixtureResponse::json(404, not_found_body());
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return FixtureResponse::empty(400);
    };
    let Some(name) = json.get("name").and_then(|value| value.as_str()) else {
        return FixtureResponse::empty(400);
    };
    let is_folder = json.get("folder").is_some();
    let child_key = join_path(parent_key, name);
    if store
        .items
        .get(&child_key)
        .is_some_and(|item| !item.deleted)
    {
        return FixtureResponse::json(
            409,
            serde_json::json!({ "error": { "code": "nameAlreadyExists" } }),
        );
    }
    let id = store.next_item_id();
    let version = store.bump_version();
    store.items.insert(
        child_key.clone(),
        Item {
            id,
            path_key: child_key.clone(),
            name: name.to_owned(),
            parent_key: Some(parent_key.to_owned()),
            is_folder,
            content: Vec::new(),
            created: Utc::now(),
            modified: Utc::now(),
            version,
            deleted: false,
        },
    );
    store.change_log.push(child_key.clone());
    let item = store.items.get(&child_key).expect("just inserted").clone();
    FixtureResponse::json(201, item_json(&item, transfer_addr))
}

fn put_content(
    store: &mut Store,
    path_key: &str,
    body: &[u8],
    transfer_addr: SocketAddr,
) -> FixtureResponse {
    let Some((parent_key, name)) = path_key.rsplit_once('/') else {
        return upsert_file(store, "", path_key, body, transfer_addr);
    };
    upsert_file(store, parent_key, name, body, transfer_addr)
}

fn upsert_file(
    store: &mut Store,
    parent_key: &str,
    name: &str,
    body: &[u8],
    transfer_addr: SocketAddr,
) -> FixtureResponse {
    let path_key = join_path(parent_key, name);
    let version = store.bump_version();
    let existing_id = store.items.get(&path_key).map(|item| item.id.clone());
    let id = existing_id.unwrap_or_else(|| store.next_item_id());
    let created = store
        .items
        .get(&path_key)
        .map_or_else(Utc::now, |item| item.created);
    store.items.insert(
        path_key.clone(),
        Item {
            id,
            path_key: path_key.clone(),
            name: name.to_owned(),
            parent_key: Some(parent_key.to_owned()),
            is_folder: false,
            content: body.to_vec(),
            created,
            modified: Utc::now(),
            version,
            deleted: false,
        },
    );
    store.change_log.push(path_key.clone());
    let item = store.items.get(&path_key).expect("just inserted").clone();
    FixtureResponse::json(200, item_json(&item, transfer_addr))
}

fn create_upload_session(
    store: &mut Store,
    path_key: &str,
    transfer_addr: SocketAddr,
) -> FixtureResponse {
    store.next_upload_id += 1;
    let session_id = format!("session-{}", store.next_upload_id);
    store.uploads.insert(
        session_id.clone(),
        PendingUpload {
            path_key: path_key.to_owned(),
            received: Vec::new(),
        },
    );
    FixtureResponse::json(
        200,
        serde_json::json!({
            "uploadUrl": format!("http://{transfer_addr}/upload-sessions/{session_id}"),
            "expirationDateTime": Utc::now().to_rfc3339(),
        }),
    )
}

fn patch_item(
    store: &mut Store,
    item_id: &str,
    body: &[u8],
    transfer_addr: SocketAddr,
) -> FixtureResponse {
    let Some(path_key) = store
        .items
        .values()
        .find(|item| item.id == item_id && !item.deleted)
        .map(|item| item.path_key.clone())
    else {
        return FixtureResponse::json(404, not_found_body());
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return FixtureResponse::empty(400);
    };
    let current = store.items.get(&path_key).expect("looked up above").clone();
    let new_name = json
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or(&current.name)
        .to_owned();
    let new_parent_key = json
        .get("parentReference")
        .and_then(|value| value.get("id"))
        .and_then(|value| value.as_str())
        .and_then(|target_id| {
            store
                .items
                .values()
                .find(|item| item.id == target_id && !item.deleted)
                .map(|item| item.path_key.clone())
        })
        .unwrap_or_else(|| current.parent_key.clone().unwrap_or_default());
    let new_path_key = join_path(&new_parent_key, &new_name);
    if new_path_key != path_key
        && store
            .items
            .get(&new_path_key)
            .is_some_and(|item| !item.deleted)
    {
        return FixtureResponse::json(
            409,
            serde_json::json!({ "error": { "code": "nameAlreadyExists" } }),
        );
    }
    let version = store.bump_version();
    let mut updated = current.clone();
    updated.name = new_name;
    updated.parent_key = Some(new_parent_key);
    updated.path_key = new_path_key.clone();
    updated.modified = Utc::now();
    updated.version = version;
    store.items.remove(&path_key);
    // Any descendants (for a folder rename/move) are rekeyed under the new
    // path so their own future lookups/deltas remain consistent.
    if updated.is_folder {
        let prefix = format!("{path_key}/");
        let descendant_keys: Vec<String> = store
            .items
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        for old_key in descendant_keys {
            if let Some(mut descendant) = store.items.remove(&old_key) {
                let rest = &old_key[prefix.len()..];
                let new_key = format!("{new_path_key}/{rest}");
                if descendant.parent_key.as_deref() == Some(path_key.as_str()) {
                    descendant.parent_key = Some(new_path_key.clone());
                }
                descendant.path_key = new_key.clone();
                store.items.insert(new_key, descendant);
            }
        }
    }
    store.items.insert(new_path_key.clone(), updated.clone());
    store.change_log.push(new_path_key);
    FixtureResponse::json(200, item_json(&updated, transfer_addr))
}

fn delete_item(store: &mut Store, item_id: &str) -> FixtureResponse {
    let Some(path_key) = store
        .items
        .values()
        .find(|item| item.id == item_id && !item.deleted)
        .map(|item| item.path_key.clone())
    else {
        return FixtureResponse::json(404, not_found_body());
    };
    let version = store.bump_version();
    if let Some(item) = store.items.get_mut(&path_key) {
        item.deleted = true;
        item.version = version;
    }
    store.change_log.push(path_key);
    FixtureResponse::empty(204)
}

fn delta_response(
    store: &mut Store,
    path_key: &str,
    query: &HashMap<String, String>,
    addrs: Addrs,
) -> FixtureResponse {
    let graph_addr = addrs.graph;
    if let Some(token) = query.get("token")
        && token == "latest"
    {
        let cursor = format!("delta_{}_{}", path_key.replace('/', "_"), store.version);
        // Echoes whatever `$top` the seed request carried into the
        // returned `deltaLink`, exactly like every other link this
        // fixture hands back - so the page size established at seed time
        // keeps flowing through every later opaque link without the
        // caller ever needing to touch one again (task 0110).
        let top_suffix = query
            .get("$top")
            .map(|top| format!("&$top={top}"))
            .unwrap_or_default();
        return FixtureResponse::json(
            200,
            serde_json::json!({
                "value": [],
                "@odata.deltaLink": format!("http://{graph_addr}/v1.0/me/drive/root/delta?cursor={cursor}{top_suffix}"),
            }),
        );
    }
    let Some(cursor) = query.get("cursor").cloned() else {
        return FixtureResponse::empty(400);
    };
    if store.expire_next_delta_poll {
        store.expire_next_delta_poll = false;
        return FixtureResponse::empty(410);
    }
    if store.expired_delta_cursors.remove(&cursor) {
        return FixtureResponse::empty(410);
    }
    let Some(since_version) = parse_cursor_version(&cursor) else {
        return FixtureResponse::empty(400);
    };
    let changed_keys: Vec<String> = store
        .change_log
        .iter()
        .filter(|key| {
            store
                .items
                .get(*key)
                .is_some_and(|item| item.version > since_version)
        })
        .cloned()
        .collect();
    let top: usize = query
        .get("$top")
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let start: usize = query
        .get("$skip")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let end = (start + top.max(1)).min(changed_keys.len());
    let page = &changed_keys[start.min(changed_keys.len())..end];
    let value: Vec<serde_json::Value> = page
        .iter()
        .filter_map(|key| store.items.get(key))
        .map(|item| {
            if item.deleted {
                serde_json::json!({ "id": item.id, "name": item.name, "deleted": { "state": "softDeleted" } })
            } else {
                item_json(item, addrs.transfer)
            }
        })
        .collect();
    let mut body = serde_json::json!({ "value": value });
    if end < changed_keys.len() {
        body["@odata.nextLink"] = serde_json::json!(format!(
            "http://{graph_addr}/v1.0/me/drive/root/delta?cursor={cursor}&$top={top}&$skip={end}"
        ));
    } else {
        let new_cursor = format!("delta_{}_{}", path_key.replace('/', "_"), store.version);
        // Echoes `$top` back for the same reason the seed branch above
        // does: this deltaLink becomes the *next* round's starting cursor,
        // and it must keep carrying the page size without this provider
        // ever having to re-attach one to an opaque link (task 0110).
        body["@odata.deltaLink"] = serde_json::json!(format!(
            "http://{graph_addr}/v1.0/me/drive/root/delta?cursor={new_cursor}&$top={top}"
        ));
    }
    FixtureResponse::json(200, body)
}

fn parse_cursor_version(cursor: &str) -> Option<u64> {
    cursor
        .rsplit_once('_')
        .and_then(|(_, version)| version.parse().ok())
}

async fn serve_transfer(
    mut stream: TcpStream,
    store: Arc<Mutex<Store>>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await? else {
        return Ok(());
    };
    let (path_only, _query) = parse_path_and_query(&request.path);
    requests.lock().await.push(CapturedRequest {
        method: request.method.clone(),
        path: request.path.clone(),
        headers: request.headers.clone(),
        body: request.body.clone(),
    });

    let mut store_guard = store.lock().await;
    let response = if let Some(id) = path_only.strip_prefix("/download/") {
        download_response(
            &store_guard,
            &percent_decode(id),
            request.headers.get("range"),
        )
    } else if let Some(session_id) = path_only.strip_prefix("/upload-sessions/") {
        upload_chunk_response(&mut store_guard, session_id, &request)
    } else {
        FixtureResponse::empty(404)
    };
    drop(store_guard);
    write_response(&mut stream, &response).await
}

fn download_response(store: &Store, item_id: &str, range: Option<&String>) -> FixtureResponse {
    let Some(item) = store
        .items
        .values()
        .find(|item| item.id == item_id && !item.deleted && !item.is_folder)
    else {
        return FixtureResponse::empty(404);
    };
    let bytes = &item.content;
    if let Some(range) = range
        && let Some((start, end)) = parse_range(range, bytes.len())
    {
        let end = end.min(bytes.len().saturating_sub(1));
        let slice = if start <= end {
            &bytes[start..=end]
        } else {
            &[]
        };
        let mut response = FixtureResponse::bytes(206, "application/octet-stream", slice.to_vec());
        response.headers.push((
            "Content-Range".to_owned(),
            format!("bytes {start}-{end}/{}", bytes.len()),
        ));
        return response;
    }
    FixtureResponse::bytes(200, "application/octet-stream", bytes.clone())
}

fn parse_range(header: &str, len: usize) -> Option<(usize, usize)> {
    let spec = header.strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: usize = start.parse().ok()?;
    let end = if end.is_empty() {
        len.saturating_sub(1)
    } else {
        end.parse().ok()?
    };
    Some((start, end))
}

fn upload_chunk_response(
    store: &mut Store,
    session_id: &str,
    request: &ParsedRequest,
) -> FixtureResponse {
    if request.method == "DELETE" {
        store.uploads.remove(session_id);
        return FixtureResponse::empty(204);
    }
    if request.method != "PUT" {
        return FixtureResponse::empty(400);
    }
    let Some(content_range) = request.headers.get("content-range") else {
        return FixtureResponse::empty(400);
    };
    let Some((start, end, total)) = parse_content_range(content_range) else {
        return FixtureResponse::empty(400);
    };
    let Some(upload) = store.uploads.get_mut(session_id) else {
        return FixtureResponse::empty(404);
    };
    if start != upload.received.len() as u64 {
        // Real Graph upload sessions require strictly sequential chunks.
        return FixtureResponse::empty(400);
    }
    if request.body.len() as u64 != end - start + 1 {
        return FixtureResponse::empty(400);
    }
    upload.received.extend_from_slice(&request.body);

    if end + 1 == total {
        let path_key = upload.path_key.clone();
        let content = upload.received.clone();
        store.uploads.remove(session_id);
        // The finished upload is only ever observed through the Graph host
        // afterwards (a fresh metadata/list request), never through this
        // response body directly, so a placeholder transfer address here is
        // safe: nothing reads this specific response's downloadUrl.
        let placeholder: SocketAddr = "127.0.0.1:1".parse().expect("valid placeholder address");
        return put_content(store, &path_key, &content, placeholder);
    }
    FixtureResponse::json(
        202,
        serde_json::json!({ "nextExpectedRanges": [format!("{}-", end + 1)] }),
    )
}

fn parse_content_range(header: &str) -> Option<(u64, u64, u64)> {
    let spec = header.strip_prefix("bytes ")?;
    let (range, total) = spec.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}
