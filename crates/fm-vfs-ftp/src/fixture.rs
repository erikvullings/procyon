//! In-process FTP server fixture for tests (task 0108).
//!
//! Mirrors [`fm_ssh::fixture`]: an isolated, real-protocol server bound to an
//! ephemeral loopback port, so provider *and* end-to-end operation-engine
//! tests exercise the actual FTP wire protocol without ever reaching an
//! external host. It implements exactly the subset of RFC 959 this workspace's
//! provider speaks (`USER`/`PASS`, `TYPE`, `PASV`/`EPSV`, `LIST`, `RETR`,
//! `STOR`, `SIZE`, `MKD`, `RMD`, `DELE`, `RNFR`/`RNTO`, `QUIT`) — deliberately
//! not a general-purpose FTP server.
//!
//! Storage is a flat path-keyed map plus a set of directory paths, which is
//! enough to model a real hierarchy: `LIST` reports only the direct children of
//! the requested directory, so nested copies behave as they would against a
//! real server.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Login name every [`FtpFixture`] accepts.
pub const FIXTURE_USERNAME: &str = "user";
/// Password every [`FtpFixture`] accepts.
pub const FIXTURE_PASSWORD: &str = "secret";

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;
type Directories = Arc<Mutex<HashSet<String>>>;

/// A running in-process plain-FTP server.
///
/// Dropping the fixture aborts its accept loop and every connection it served.
pub struct FtpFixture {
    /// Loopback address the fixture is listening on.
    pub addr: SocketAddr,
    files: Files,
    directories: Directories,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FtpFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FtpFixture {
    /// Starts a fixture on an ephemeral loopback port with an empty root.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the fixture must bind a loopback port");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its address");
        let files: Files = Arc::new(Mutex::new(HashMap::new()));
        let directories: Directories = Arc::new(Mutex::new(HashSet::from(["/".to_owned()])));
        let served_files = Arc::clone(&files);
        let served_directories = Arc::clone(&directories);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let files = Arc::clone(&served_files);
                let directories = Arc::clone(&served_directories);
                tokio::spawn(async move { serve(stream, files, directories).await });
            }
        });
        Self {
            addr,
            files,
            directories,
            task,
        }
    }

    /// Seeds a file at an absolute remote path, e.g. `/report.txt`.
    pub async fn put(&self, path: &str, body: &[u8]) {
        self.files
            .lock()
            .await
            .insert(path.to_owned(), body.to_vec());
    }

    /// Seeds a directory at an absolute remote path, e.g. `/downloads`.
    pub async fn create_directory(&self, path: &str) {
        self.directories.lock().await.insert(path.to_owned());
    }

    /// Reads a file back, or `None` when it does not exist.
    pub async fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().await.get(path).cloned()
    }

    /// Returns every stored file path, sorted, for leftover-temporary
    /// assertions.
    pub async fn paths(&self) -> Vec<String> {
        let mut paths: Vec<_> = self.files.lock().await.keys().cloned().collect();
        paths.sort();
        paths
    }
}

/// Splits an absolute path into its parent directory and its basename.
fn split_path(path: &str) -> (String, String) {
    match path.rsplit_once('/') {
        Some(("", name)) => ("/".to_owned(), name.to_owned()),
        Some((parent, name)) => (parent.to_owned(), name.to_owned()),
        None => ("/".to_owned(), path.to_owned()),
    }
}

/// Normalizes a `LIST` argument so `/`, `` and `/nested/` all compare equal to
/// the stored directory paths.
fn normalize_directory(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
}

async fn listing(directory: &str, files: &Files, directories: &Directories) -> String {
    let directory = normalize_directory(directory);
    let mut listing = String::new();
    for path in directories.lock().await.iter() {
        if path == "/" {
            continue;
        }
        let (parent, name) = split_path(path);
        if parent == directory {
            listing.push_str(&format!(
                "drwxr-xr-x 1 owner group 0 Jan 01 2024 {name}\r\n"
            ));
        }
    }
    for (path, body) in files.lock().await.iter() {
        let (parent, name) = split_path(path);
        if parent == directory {
            listing.push_str(&format!(
                "-rw-r--r-- 1 owner group {} Jan 01 2024 {name}\r\n",
                body.len()
            ));
        }
    }
    listing
}

#[allow(clippy::too_many_lines)]
async fn serve(stream: TcpStream, files: Files, directories: Directories) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut data_listener: Option<TcpListener> = None;
    let mut rename_from: Option<String> = None;
    if write.write_all(b"220 fm fixture\r\n").await.is_err() {
        return;
    }
    while let Ok(Some(line)) = lines.next_line().await {
        let (command, argument) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let reply: Result<(), std::io::Error> = match command.to_ascii_uppercase().as_str() {
            "USER" => write.write_all(b"331 password required\r\n").await,
            "PASS" => write.write_all(b"230 logged in\r\n").await,
            "TYPE" | "NOOP" => write.write_all(b"200 ok\r\n").await,
            "EPSV" => match TcpListener::bind("127.0.0.1:0").await {
                Ok(listener) => {
                    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
                    data_listener = Some(listener);
                    write
                        .write_all(
                            format!("229 Entering Extended Passive Mode (|||{port}|)\r\n")
                                .as_bytes(),
                        )
                        .await
                }
                Err(_) => write.write_all(b"425 cannot open data port\r\n").await,
            },
            "PASV" => match TcpListener::bind("127.0.0.1:0").await {
                Ok(listener) => {
                    let port = listener.local_addr().map(|addr| addr.port()).unwrap_or(0);
                    data_listener = Some(listener);
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
                }
                Err(_) => write.write_all(b"425 cannot open data port\r\n").await,
            },
            "LIST" => {
                let body = listing(argument, &files, &directories).await;
                if write.write_all(b"150 opening data\r\n").await.is_err() {
                    return;
                }
                let Some(listener) = data_listener.take() else {
                    return;
                };
                if let Ok((mut data, _)) = listener.accept().await {
                    let _ = data.write_all(body.as_bytes()).await;
                    let _ = data.shutdown().await;
                }
                write.write_all(b"226 transfer complete\r\n").await
            }
            "SIZE" => match files.lock().await.get(argument).map(Vec::len) {
                Some(size) => write.write_all(format!("213 {size}\r\n").as_bytes()).await,
                None => write.write_all(b"550 not found\r\n").await,
            },
            "RETR" => {
                let body = files.lock().await.get(argument).cloned();
                match body {
                    Some(body) => {
                        if write.write_all(b"150 opening data\r\n").await.is_err() {
                            return;
                        }
                        let Some(listener) = data_listener.take() else {
                            return;
                        };
                        if let Ok((mut data, _)) = listener.accept().await {
                            let _ = data.write_all(&body).await;
                            let _ = data.shutdown().await;
                        }
                        write.write_all(b"226 transfer complete\r\n").await
                    }
                    None => write.write_all(b"550 not found\r\n").await,
                }
            }
            "STOR" => {
                if write.write_all(b"150 opening data\r\n").await.is_err() {
                    return;
                }
                let Some(listener) = data_listener.take() else {
                    return;
                };
                if let Ok((mut data, _)) = listener.accept().await {
                    let mut body = Vec::new();
                    if data.read_to_end(&mut body).await.is_ok() {
                        files.lock().await.insert(argument.to_owned(), body);
                    }
                }
                write.write_all(b"226 transfer complete\r\n").await
            }
            "MKD" => {
                directories.lock().await.insert(argument.to_owned());
                write.write_all(b"257 created\r\n").await
            }
            "RNFR" => {
                rename_from = Some(argument.to_owned());
                write.write_all(b"350 ready\r\n").await
            }
            "RNTO" => match rename_from.take() {
                Some(from) => {
                    let mut files = files.lock().await;
                    match files.remove(&from) {
                        Some(body) => {
                            files.insert(argument.to_owned(), body);
                            drop(files);
                            write.write_all(b"250 renamed\r\n").await
                        }
                        None => {
                            drop(files);
                            write.write_all(b"550 not found\r\n").await
                        }
                    }
                }
                None => write.write_all(b"503 bad sequence\r\n").await,
            },
            "DELE" => {
                if files.lock().await.remove(argument).is_some() {
                    write.write_all(b"250 deleted\r\n").await
                } else {
                    write.write_all(b"550 not found\r\n").await
                }
            }
            "RMD" => {
                if directories.lock().await.remove(argument) {
                    write.write_all(b"250 deleted\r\n").await
                } else {
                    write.write_all(b"550 not found\r\n").await
                }
            }
            "QUIT" => {
                let _ = write.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => write.write_all(b"502 unsupported\r\n").await,
        };
        if reply.is_err() {
            return;
        }
    }
}
