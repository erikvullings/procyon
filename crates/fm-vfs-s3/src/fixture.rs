//! In-process S3-compatible mock server for tests (task 0146), following the
//! same "real protocol, ephemeral loopback port, no docker" convention as
//! [`fm_vfs_ftp`](https://docs.rs/fm-vfs-ftp)'s `FtpFixture` and
//! `fm-ssh`'s `SshFixture`: this workspace runs integration tests against a
//! hand-rolled server speaking the wire protocol it targets rather than a
//! container, so CI never needs Docker or real cloud credentials.
//!
//! This fixture does not verify SigV4 signatures - it implements enough of
//! the S3 REST API (path-style bucket/key routing, `ListObjectsV2`
//! delimiter/prefix semantics, ranged `GetObject`, `PutObject`, `CopyObject`
//! via `PutObject` + `x-amz-copy-source`, `DeleteObject`, and the multipart
//! upload trio) to exercise [`crate::S3FileSystemProvider`] end to end.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

/// Access key id accepted by [`S3Fixture`] (no signature verification, so
/// any value works; tests use this one for clarity).
pub const FIXTURE_ACCESS_KEY_ID: &str = "fixture-access-key";
/// Secret access key accepted by [`S3Fixture`].
pub const FIXTURE_SECRET_ACCESS_KEY: &str = "fixture-secret-key";
/// Bucket name [`S3Fixture`] serves.
pub const FIXTURE_BUCKET: &str = "fixture-bucket";
/// Region [`S3Fixture`] reports.
pub const FIXTURE_REGION: &str = "us-east-1";

#[derive(Default)]
struct Store {
    objects: HashMap<String, Vec<u8>>,
    uploads: HashMap<String, HashMap<u16, Vec<u8>>>,
}

/// A running in-process mock S3-compatible endpoint.
pub struct S3Fixture {
    /// `http://127.0.0.1:<port>`, suitable as
    /// [`crate::S3ConnectionParameters::endpoint`].
    pub endpoint: String,
    shutdown: watch::Sender<bool>,
}

impl S3Fixture {
    /// Starts the fixture on an ephemeral loopback port.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("binding an ephemeral loopback port must succeed");
        let addr = listener
            .local_addr()
            .expect("a bound listener has a local address");
        let store = Arc::new(Mutex::new(Store::default()));
        let (shutdown, mut shutdown_rx) = watch::channel(false);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { continue };
                        let store = store.clone();
                        tokio::spawn(async move {
                            let _ = serve_one(stream, store).await;
                        });
                    }
                    _ = shutdown_rx.changed() => break,
                }
            }
        });

        Self {
            endpoint: format!("http://{addr}"),
            shutdown,
        }
    }
}

impl Drop for S3Fixture {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

async fn serve_one(
    mut stream: tokio::net::TcpStream,
    store: Arc<Mutex<Store>>,
) -> std::io::Result<()> {
    let request = read_request(&mut stream).await?;
    let response = handle(&request, &store);
    write_response(&mut stream, &response).await
}

struct Request {
    method: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    reason: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = find_subslice(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break buffer.len();
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let header_text = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let (path, query) = parse_target(&target);

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

    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    response: &Response,
) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\nContent-Length: {}\r\n",
        response.status,
        response.reason,
        response.body.len()
    );
    for (name, value) in &response.headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes()).await?;
    stream.write_all(&response.body).await?;
    stream.flush().await
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_target(target: &str) -> (String, HashMap<String, String>) {
    let (path, query_string) = target.split_once('?').unwrap_or((target, ""));
    let mut query = HashMap::new();
    for pair in query_string.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(percent_decode(key), percent_decode(value));
    }
    (percent_decode(path), query)
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
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Splits a path-style request path (`/bucket/key/with/slashes`) into the
/// object key (empty for the bucket root).
fn object_key(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    match trimmed.split_once('/') {
        Some((_bucket, key)) => key.to_owned(),
        None => String::new(),
    }
}

fn handle(request: &Request, store: &Arc<Mutex<Store>>) -> Response {
    let key = object_key(&request.path);
    let mut store = store.lock().expect("fixture store mutex is never poisoned");

    match (request.method.as_str(), key.is_empty()) {
        ("GET", true) if request.query.contains_key("list-type") => list_objects(request, &store),
        ("GET", false) => get_object(request, &key, &store),
        ("PUT", false) if request.query.contains_key("uploadId") => {
            upload_part(request, &key, &mut store)
        }
        ("PUT", false) if request.headers.contains_key("x-amz-copy-source") => {
            copy_object(request, &key, &mut store)
        }
        ("PUT", false) => put_object(request, &key, &mut store),
        ("DELETE", false) if request.query.contains_key("uploadId") => {
            abort_multipart_upload(request, &mut store)
        }
        ("DELETE", false) => delete_object(&key, &mut store),
        ("POST", false) if request.query.contains_key("uploads") => {
            create_multipart_upload(&key, &mut store)
        }
        ("POST", false) if request.query.contains_key("uploadId") => {
            complete_multipart_upload(request, &key, &mut store)
        }
        _ => Response {
            status: 400,
            reason: "Bad Request",
            headers: Vec::new(),
            body: Vec::new(),
        },
    }
}

fn list_objects(request: &Request, store: &Store) -> Response {
    let prefix = request.query.get("prefix").cloned().unwrap_or_default();
    let delimiter = request.query.get("delimiter").cloned();
    let continuation_token = request.query.get("continuation-token").cloned();
    let max_keys: usize = request
        .query
        .get("max-keys")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);

    let mut keys: Vec<&String> = store
        .objects
        .keys()
        .filter(|key| key.starts_with(&prefix))
        .collect();
    keys.sort();

    let mut common_prefixes: Vec<String> = Vec::new();
    let mut contents: Vec<&String> = Vec::new();
    for key in &keys {
        let rest = &key[prefix.len()..];
        if let Some(delimiter) = &delimiter
            && let Some(position) = rest.find(delimiter.as_str())
        {
            let common = format!("{prefix}{}", &rest[..position + delimiter.len()]);
            if !common_prefixes.contains(&common) {
                common_prefixes.push(common);
            }
            continue;
        }
        contents.push(key);
    }
    common_prefixes.sort();

    let start_index = continuation_token
        .as_deref()
        .and_then(|token| token.parse::<usize>().ok())
        .unwrap_or(0);
    let mut items: Vec<Item> = common_prefixes
        .into_iter()
        .map(Item::Prefix)
        .chain(contents.into_iter().map(|key| Item::Object(key.clone())))
        .collect();
    items.sort_by(|a, b| a.sort_key().cmp(b.sort_key()));

    let page: Vec<&Item> = items.iter().skip(start_index).take(max_keys).collect();
    let next_index = start_index + page.len();
    let has_more = next_index < items.len();

    let mut body = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">",
    );
    for item in &page {
        match item {
            Item::Prefix(prefix) => {
                body.push_str(&format!(
                    "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
                    xml_escape(prefix)
                ));
            }
            Item::Object(key) => {
                let size = store.objects.get(key).map(Vec::len).unwrap_or(0);
                body.push_str(&format!(
                    "<Contents><Key>{}</Key><Size>{size}</Size><LastModified>2026-01-01T00:00:00.000Z</LastModified><ETag>&quot;fixture&quot;</ETag></Contents>",
                    xml_escape(key)
                ));
            }
        }
    }
    if has_more {
        body.push_str(&format!(
            "<NextContinuationToken>{next_index}</NextContinuationToken>"
        ));
    }
    body.push_str("</ListBucketResult>");

    Response {
        status: 200,
        reason: "OK",
        headers: vec![("Content-Type".to_owned(), "application/xml".to_owned())],
        body: body.into_bytes(),
    }
}

enum Item {
    Prefix(String),
    Object(String),
}

impl Item {
    fn sort_key(&self) -> &str {
        match self {
            Self::Prefix(value) | Self::Object(value) => value,
        }
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn get_object(request: &Request, key: &str, store: &Store) -> Response {
    let Some(bytes) = store.objects.get(key) else {
        return not_found();
    };
    if let Some(range) = request.headers.get("range")
        && let Some((start, end)) = parse_range(range, bytes.len())
    {
        let slice = &bytes[start..=end.min(bytes.len().saturating_sub(1))];
        return Response {
            status: 206,
            reason: "Partial Content",
            headers: vec![(
                "Content-Range".to_owned(),
                format!("bytes {start}-{end}/{}", bytes.len()),
            )],
            body: slice.to_vec(),
        };
    }
    Response {
        status: 200,
        reason: "OK",
        headers: Vec::new(),
        body: bytes.clone(),
    }
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

fn put_object(request: &Request, key: &str, store: &mut Store) -> Response {
    store.objects.insert(key.to_owned(), request.body.clone());
    Response {
        status: 200,
        reason: "OK",
        headers: vec![("ETag".to_owned(), "\"fixture\"".to_owned())],
        body: Vec::new(),
    }
}

fn copy_object(request: &Request, destination_key: &str, store: &mut Store) -> Response {
    let source_header = request
        .headers
        .get("x-amz-copy-source")
        .cloned()
        .unwrap_or_default();
    let source_key = object_key(&percent_decode(&source_header));
    let Some(bytes) = store.objects.get(&source_key).cloned() else {
        return not_found();
    };
    store.objects.insert(destination_key.to_owned(), bytes);
    Response {
        status: 200,
        reason: "OK",
        headers: vec![("Content-Type".to_owned(), "application/xml".to_owned())],
        body: b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><CopyObjectResult><ETag>&quot;fixture&quot;</ETag></CopyObjectResult>".to_vec(),
    }
}

fn delete_object(key: &str, store: &mut Store) -> Response {
    store.objects.remove(key);
    Response {
        status: 204,
        reason: "No Content",
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn create_multipart_upload(key: &str, store: &mut Store) -> Response {
    let upload_id = format!("upload-{}", store.uploads.len() + 1);
    store.uploads.insert(upload_id.clone(), HashMap::new());
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Bucket>{}</Bucket><Key>{}</Key><UploadId>{upload_id}</UploadId></InitiateMultipartUploadResult>",
        xml_escape(FIXTURE_BUCKET),
        xml_escape(key)
    );
    Response {
        status: 200,
        reason: "OK",
        headers: vec![("Content-Type".to_owned(), "application/xml".to_owned())],
        body: body.into_bytes(),
    }
}

fn upload_part(request: &Request, _key: &str, store: &mut Store) -> Response {
    let Some(upload_id) = request.query.get("uploadId") else {
        return bad_request();
    };
    let Some(part_number) = request
        .query
        .get("partNumber")
        .and_then(|value| value.parse::<u16>().ok())
    else {
        return bad_request();
    };
    let Some(parts) = store.uploads.get_mut(upload_id) else {
        return not_found();
    };
    parts.insert(part_number, request.body.clone());
    Response {
        status: 200,
        reason: "OK",
        headers: vec![("ETag".to_owned(), format!("\"part-{part_number}\""))],
        body: Vec::new(),
    }
}

fn complete_multipart_upload(request: &Request, key: &str, store: &mut Store) -> Response {
    let Some(upload_id) = request.query.get("uploadId") else {
        return bad_request();
    };
    let Some(parts) = store.uploads.remove(upload_id) else {
        return not_found();
    };
    let mut numbers: Vec<u16> = parts.keys().copied().collect();
    numbers.sort_unstable();
    let mut assembled = Vec::new();
    for number in numbers {
        assembled.extend_from_slice(&parts[&number]);
    }
    store.objects.insert(key.to_owned(), assembled);
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Bucket>{}</Bucket><Key>{}</Key><ETag>&quot;fixture&quot;</ETag></CompleteMultipartUploadResult>",
        xml_escape(FIXTURE_BUCKET),
        xml_escape(key)
    );
    Response {
        status: 200,
        reason: "OK",
        headers: vec![("Content-Type".to_owned(), "application/xml".to_owned())],
        body: body.into_bytes(),
    }
}

fn abort_multipart_upload(request: &Request, store: &mut Store) -> Response {
    if let Some(upload_id) = request.query.get("uploadId") {
        store.uploads.remove(upload_id);
    }
    Response {
        status: 204,
        reason: "No Content",
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn not_found() -> Response {
    Response {
        status: 404,
        reason: "Not Found",
        headers: vec![("Content-Type".to_owned(), "application/xml".to_owned())],
        body: b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>NoSuchKey</Code></Error>"
            .to_vec(),
    }
}

fn bad_request() -> Response {
    Response {
        status: 400,
        reason: "Bad Request",
        headers: Vec::new(),
        body: Vec::new(),
    }
}
