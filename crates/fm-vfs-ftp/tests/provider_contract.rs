//! Wire-level contracts for the FTP provider.

#![allow(clippy::unwrap_used)]

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

use async_trait::async_trait;
use fm_domain::Location;
use fm_vfs::{
    CONSERVATIVE_POLL_INTERVAL, ChangeTracking, EntryRef, FileSystemProvider, ListOptions,
    ProviderCapabilities, VfsError,
};
use fm_vfs_ftp::{FtpConnectionParameters, FtpConnectionResolver, FtpFileSystemProvider};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

struct Resolver {
    secure: bool,
    port: u16,
}

struct RebexResolver {
    explicit_tls: bool,
}

#[async_trait]
impl FtpConnectionResolver for RebexResolver {
    async fn resolve(&self, _id: &str) -> Result<FtpConnectionParameters, VfsError> {
        Ok(FtpConnectionParameters {
            host: "test.rebex.net".to_owned(),
            port: 21,
            username: "demo".to_owned(),
            password: "password".to_owned(),
            explicit_tls: self.explicit_tls,
        })
    }
}

#[async_trait]
impl FtpConnectionResolver for Resolver {
    async fn resolve(&self, _id: &str) -> Result<FtpConnectionParameters, VfsError> {
        Ok(FtpConnectionParameters {
            host: "127.0.0.1".to_owned(),
            port: self.port,
            username: "user".to_owned(),
            password: "secret".to_owned(),
            explicit_tls: self.secure,
        })
    }
}

fn provider(secure: bool) -> FtpFileSystemProvider {
    provider_at(secure, 1)
}

fn provider_at(secure: bool, port: u16) -> FtpFileSystemProvider {
    FtpFileSystemProvider::new(Arc::new(Resolver { secure, port }))
}

struct Fixture {
    addr: SocketAddr,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let files = Arc::new(Mutex::new(HashMap::from([(
            "/hello.txt".into(),
            b"hello".to_vec(),
        )])));
        let dirs = Arc::new(Mutex::new(HashSet::from(["/".to_owned()])));
        let shared_files = files.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let files = shared_files.clone();
                let dirs = dirs.clone();
                tokio::spawn(async move {
                    serve(stream, files, dirs).await;
                });
            }
        });
        Self { addr, files, task }
    }
}

async fn start_untrusted_ftps_fixture() -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).unwrap();
    let config = tokio_rustls::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            tokio_rustls::rustls::pki_types::PrivateKeyDer::Pkcs8(
                certificate.signing_key.serialize_der().into(),
            ),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        stream
            .write_all(b"220 isolated FTPS fixture\r\n")
            .await
            .unwrap();
        let mut line = Vec::new();
        loop {
            let mut byte = [0];
            if stream.read_exact(&mut byte).await.is_err() {
                return;
            }
            line.push(byte[0]);
            if line.ends_with(b"\r\n") {
                break;
            }
        }
        assert_eq!(String::from_utf8_lossy(&line).trim(), "AUTH TLS");
        stream.write_all(b"234 begin TLS\r\n").await.unwrap();
        let _ = acceptor.accept(stream).await;
    });
    (addr, task)
}

async fn serve(
    stream: TcpStream,
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    dirs: Arc<Mutex<HashSet<String>>>,
) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut data_listener: Option<TcpListener> = None;
    let mut rename_from = None;
    write.write_all(b"220 isolated fixture\r\n").await.unwrap();
    while let Ok(Some(line)) = lines.next_line().await {
        let (command, argument) = line.split_once(' ').unwrap_or((&line, ""));
        match command.to_ascii_uppercase().as_str() {
            "USER" => write.write_all(b"331 password required\r\n").await.unwrap(),
            "PASS" => write.write_all(b"230 logged in\r\n").await.unwrap(),
            "TYPE" | "NOOP" => write.write_all(b"200 ok\r\n").await.unwrap(),
            "EPSV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                write
                    .write_all(
                        format!("229 Entering Extended Passive Mode (|||{port}|)\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                data_listener = Some(listener);
            }
            "PASV" => {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                write
                    .write_all(
                        format!(
                            "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                            port / 256,
                            port % 256
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                data_listener = Some(listener);
            }
            "LIST" => {
                write.write_all(b"150 opening data\r\n").await.unwrap();
                let (mut data, _) = data_listener.take().unwrap().accept().await.unwrap();
                let mut listing = String::new();
                for dir in dirs.lock().await.iter().filter(|p| p.as_str() != "/") {
                    listing.push_str(&format!(
                        "drwxr-xr-x 1 owner group 0 Jan 01 2024 {}\r\n",
                        dir.trim_start_matches('/')
                    ));
                }
                for (path, body) in files.lock().await.iter() {
                    listing.push_str(&format!(
                        "-rw-r--r-- 1 owner group {} Jan 01 2024 {}\r\n",
                        body.len(),
                        path.trim_start_matches('/')
                    ));
                }
                data.write_all(listing.as_bytes()).await.unwrap();
                drop(data);
                write.write_all(b"226 transfer complete\r\n").await.unwrap();
            }
            "SIZE" => match files.lock().await.get(argument).map(Vec::len) {
                Some(size) => write
                    .write_all(format!("213 {size}\r\n").as_bytes())
                    .await
                    .unwrap(),
                None => write.write_all(b"550 not found\r\n").await.unwrap(),
            },
            "RETR" => {
                let body = files.lock().await.get(argument).cloned();
                if let Some(body) = body {
                    write.write_all(b"150 opening data\r\n").await.unwrap();
                    let (mut data, _) = data_listener.take().unwrap().accept().await.unwrap();
                    data.write_all(&body).await.unwrap();
                    drop(data);
                    write.write_all(b"226 transfer complete\r\n").await.unwrap();
                } else {
                    write.write_all(b"550 not found\r\n").await.unwrap();
                }
            }
            "STOR" => {
                write.write_all(b"150 opening data\r\n").await.unwrap();
                let (mut data, _) = data_listener.take().unwrap().accept().await.unwrap();
                let mut body = Vec::new();
                data.read_to_end(&mut body).await.unwrap();
                files.lock().await.insert(argument.to_owned(), body);
                write.write_all(b"226 transfer complete\r\n").await.unwrap();
            }
            "MKD" => {
                dirs.lock().await.insert(argument.to_owned());
                write.write_all(b"257 created\r\n").await.unwrap();
            }
            "RNFR" => {
                rename_from = Some(argument.to_owned());
                write.write_all(b"350 ready\r\n").await.unwrap();
            }
            "RNTO" => {
                let from = rename_from.take().unwrap();
                let mut files = files.lock().await;
                if let Some(body) = files.remove(&from) {
                    files.insert(argument.to_owned(), body);
                }
                write.write_all(b"250 renamed\r\n").await.unwrap();
            }
            "DELE" => {
                files.lock().await.remove(argument);
                write.write_all(b"250 deleted\r\n").await.unwrap();
            }
            "RMD" => {
                dirs.lock().await.remove(argument);
                write.write_all(b"250 deleted\r\n").await.unwrap();
            }
            "QUIT" => {
                write.write_all(b"221 bye\r\n").await.unwrap();
                break;
            }
            _ => write.write_all(b"502 unsupported\r\n").await.unwrap(),
        }
    }
}

#[test]
fn reports_only_implemented_ftp_capabilities() {
    let capabilities = provider(false).capabilities();
    for supported in [
        ProviderCapabilities::LIST,
        ProviderCapabilities::READ,
        ProviderCapabilities::WRITE,
        ProviderCapabilities::CREATE_DIRECTORY,
        ProviderCapabilities::RENAME,
        ProviderCapabilities::MOVE,
        ProviderCapabilities::DELETE,
    ] {
        assert!(capabilities.contains(supported));
    }
    for unsupported in [
        ProviderCapabilities::WATCH,
        ProviderCapabilities::CHECKSUM,
        ProviderCapabilities::SERVER_SIDE_COPY,
        ProviderCapabilities::SET_TIMESTAMPS,
        ProviderCapabilities::SET_PERMISSIONS,
        ProviderCapabilities::TRASH,
    ] {
        assert!(!capabilities.contains(unsupported));
    }
}

/// Task 0108: the endpoint must identify the concrete connection (id *and*
/// transport security), never just the `ftp` provider type.
#[test]
fn transfer_capabilities_identify_the_connection_rather_than_the_provider_type() {
    let provider = provider(false);
    let first = provider
        .transfer_capabilities(
            &Location::parse("ftp://11111111-1111-4111-8111-111111111111/a.txt").unwrap(),
        )
        .unwrap();
    let same_connection = provider
        .transfer_capabilities(
            &Location::parse("ftp://11111111-1111-4111-8111-111111111111/nested/b.txt").unwrap(),
        )
        .unwrap();
    let other_connection = provider
        .transfer_capabilities(
            &Location::parse("ftp://22222222-2222-4222-8222-222222222222/a.txt").unwrap(),
        )
        .unwrap();
    let same_id_over_tls = provider
        .transfer_capabilities(
            &Location::parse("ftps://11111111-1111-4111-8111-111111111111/a.txt").unwrap(),
        )
        .unwrap();

    assert!(first.shares_endpoint_with(&same_connection));
    assert!(!first.shares_endpoint_with(&other_connection));
    assert!(!first.shares_endpoint_with(&same_id_over_tls));
    // FTP has no server-side copy, but `RNFR`/`RNTO` is a real server-side move.
    assert!(!first.server_side_copy);
    assert!(first.server_side_move);
    assert!(!first.random_read);
    assert!(!first.random_write);
    assert!(!first.resumable_upload);
    assert!(!first.resumable_download);
}

#[test]
fn transfer_capabilities_reject_a_malformed_location() {
    let result = provider(false).transfer_capabilities(&Location::new(
        fm_domain::ProviderId::new("ftp"),
        "ftp://not-a-uuid/a.txt",
    ));

    assert!(matches!(result, Err(VfsError::InvalidLocation { .. })));
}

#[test]
fn change_tracking_reports_conservative_polling_rather_than_the_native_watch_default() {
    assert_eq!(
        provider(false).change_tracking(),
        ChangeTracking::Poll {
            interval: CONSERVATIVE_POLL_INTERVAL
        }
    );
}

#[tokio::test]
async fn cancellation_prevents_a_network_operation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let location = Location::parse("ftp://11111111-1111-4111-8111-111111111111/").unwrap();
    let result = provider(false)
        .list(&location, ListOptions::default(), cancellation)
        .await;
    assert!(matches!(result, Err(VfsError::Cancelled)));
}

#[tokio::test]
async fn location_security_must_match_saved_connection_kind() {
    let location = Location::parse("ftps://11111111-1111-4111-8111-111111111111/").unwrap();
    let result = provider(false)
        .list(&location, ListOptions::default(), CancellationToken::new())
        .await;
    assert!(matches!(result, Err(VfsError::InvalidLocation { .. })));
}

#[tokio::test]
async fn explicit_ftps_rejects_an_untrusted_server_certificate() {
    let (addr, task) = start_untrusted_ftps_fixture().await;
    let parameters = FtpConnectionParameters {
        host: "localhost".to_owned(),
        port: addr.port(),
        username: "user".to_owned(),
        password: "secret".to_owned(),
        explicit_tls: true,
    };
    let result = FtpFileSystemProvider::verify_connectivity(&parameters).await;
    task.abort();
    assert!(matches!(result, Err(VfsError::Io { .. })));
}

#[tokio::test]
#[ignore = "public Rebex smoke test; requires internet access"]
async fn rebex_ftp_lists_and_reads_its_public_example_file() {
    run_rebex_smoke(false).await;
}

#[tokio::test]
#[ignore = "public Rebex smoke test; requires internet access"]
async fn rebex_explicit_ftps_lists_and_reads_its_public_example_file() {
    run_rebex_smoke(true).await;
}

async fn run_rebex_smoke(explicit_tls: bool) {
    if let Err(message) = try_rebex_smoke(explicit_tls).await {
        if std::env::var_os("FM_REBEX_STRICT").is_some() {
            panic!("{message}");
        }
        eprintln!("Rebex smoke test skipped: {message}");
    }
}

async fn try_rebex_smoke(explicit_tls: bool) -> Result<(), String> {
    let provider = FtpFileSystemProvider::new(Arc::new(RebexResolver { explicit_tls }));
    let scheme = if explicit_tls { "ftps" } else { "ftp" };
    let directory = Location::parse(&format!(
        "{scheme}://11111111-1111-4111-8111-111111111111/pub/example"
    ))
    .map_err(|error| error.to_string())?;
    let page = provider
        .list(&directory, ListOptions::default(), CancellationToken::new())
        .await
        .map_err(|error| format!("{scheme} listing failed: {error}"))?;
    let readme = page
        .entries
        .iter()
        .find(|entry| entry.name == "readme.txt")
        .ok_or_else(|| format!("{scheme} listing did not contain readme.txt"))?;
    let mut reader = provider
        .open_read(
            &EntryRef {
                id: readme.id,
                location: readme.location.clone(),
            },
            CancellationToken::new(),
        )
        .await
        .map_err(|error| format!("{scheme} readme.txt open failed: {error}"))?;
    let mut contents = Vec::new();
    reader
        .read_to_end(&mut contents)
        .await
        .map_err(|error| format!("{scheme} readme.txt download failed: {error}"))?;
    if contents.is_empty() {
        return Err(format!("{scheme} readme.txt was empty"));
    }
    Ok(())
}

#[tokio::test]
async fn passive_ftp_fixture_supports_the_file_workflow() {
    let fixture = Fixture::start().await;
    let provider = provider_at(false, fixture.addr.port());
    let root = Location::parse("ftp://11111111-1111-4111-8111-111111111111/").unwrap();
    let cancellation = CancellationToken::new();

    let listed = provider
        .list(&root, ListOptions::default(), cancellation.clone())
        .await
        .unwrap();
    assert!(listed.entries.iter().any(|entry| entry.name == "hello.txt"));

    let upload = root.join("upload.txt").unwrap();
    let mut writer = provider
        .open_write(
            &upload,
            fm_vfs::WriteOptions { overwrite: true },
            cancellation.clone(),
        )
        .await
        .unwrap();
    writer.write_all(b"uploaded").await.unwrap();
    writer.shutdown().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !fixture.files.lock().await.contains_key("/upload.txt") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fixture.files.lock().await.get("/upload.txt").unwrap(),
        b"uploaded"
    );

    let mut reader = provider
        .open_read(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: upload.clone(),
            },
            cancellation.clone(),
        )
        .await
        .unwrap();
    let mut downloaded = Vec::new();
    reader.read_to_end(&mut downloaded).await.unwrap();
    assert_eq!(downloaded, b"uploaded");

    provider
        .create_directory(&root, "folder", cancellation.clone())
        .await
        .unwrap();
    let moved = root.join("moved.txt").unwrap();
    provider
        .rename(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: upload,
            },
            &moved,
            cancellation.clone(),
        )
        .await
        .unwrap();
    provider
        .remove(
            &EntryRef {
                id: fm_domain::EntryId::new(),
                location: moved,
            },
            fm_vfs::RemoveOptions::default(),
            cancellation,
        )
        .await
        .unwrap();
    assert!(!fixture.files.lock().await.contains_key("/moved.txt"));
}
