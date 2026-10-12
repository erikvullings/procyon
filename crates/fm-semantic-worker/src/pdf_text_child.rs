//! Opt-in, text-only converter child for bounded PDF source streams.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, PdfPageWindow, PdfTextReport, SourceContent, merge_pdf_text_windows,
};
use sha2::{Digest, Sha256};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio_util::sync::CancellationToken;

use crate::ingestion::PdfTextSource;

const MAX_SOURCE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_BYTES: u64 = 16 * 1024;
// Experimental ceiling pending four-target installed qualification.
const MAX_SAMPLED_RSS: u64 = 512 * 1024 * 1024;
const CHILD_DEADLINE: Duration = Duration::from_secs(120);

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Runs a killable converter without copying the anonymous spool into the parent.
///
/// This experimental path does not replace production ingestion until the
/// installed four-target watchdog and atomic-publish behavior are qualified.
pub fn convert_spool(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    cancellation: &CancellationToken,
) -> io::Result<fm_semantic_conversion::ConvertedDocument> {
    let length = spool.metadata()?.len();
    if length == 0 || length > MAX_SOURCE_BYTES {
        return Err(io::Error::other("PDF spool exceeds child source limit"));
    }

    let mut input = spool.try_clone()?;
    input.seek(SeekFrom::Start(0))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 1024 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(io::Error::other("PDF conversion cancelled"));
        }
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let actual = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected_sha256 || input.metadata()?.len() != length {
        return Err(io::Error::other(
            "PDF spool digest does not match the source",
        ));
    }
    input.seek(SeekFrom::Start(0))?;
    let mut child = KillOnDrop(
        Command::new(executable)
            .arg("--pdf-text-child")
            .arg(expected_sha256)
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child output"))?;
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(MAX_REPORT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok::<_, io::Error>(bytes)
    });
    let mut stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("missing child error"))?;
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .by_ref()
            .take(MAX_ERROR_BYTES + 1)
            .read_to_end(&mut bytes)?;
        Ok::<_, io::Error>(bytes)
    });
    let started = Instant::now();
    let pid = Pid::from_u32(child.0.id());
    let mut system = System::new();
    let mut sampled = false;
    loop {
        if cancellation.is_cancelled() {
            return Err(io::Error::other("PDF conversion cancelled"));
        }
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_memory(),
        );
        if let Some(process) = system.process(pid) {
            sampled = true;
            if process.memory() > MAX_SAMPLED_RSS {
                return Err(io::Error::other(
                    "PDF converter exceeded sampled memory ceiling",
                ));
            }
        } else if child.0.try_wait()?.is_none() && started.elapsed() > Duration::from_millis(100) {
            return Err(io::Error::other(
                "PDF converter memory could not be sampled",
            ));
        }
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                return Err(io::Error::other("PDF converter child failed"));
            }
            if !sampled {
                return Err(io::Error::other("PDF converter had no memory sample"));
            }
            let bytes = output
                .join()
                .map_err(|_| io::Error::other("PDF converter output reader failed"))??;
            errors
                .join()
                .map_err(|_| io::Error::other("PDF converter error reader failed"))??;
            if bytes.len() > MAX_REPORT_BYTES {
                return Err(io::Error::other(
                    "PDF converter output exceeded its byte limit",
                ));
            }
            let report: PdfTextReport = serde_json::from_slice(&bytes)
                .map_err(|_| io::Error::other("PDF converter output was invalid"))?;
            return report
                .into_document(&ConversionBudgets {
                    max_source_bytes: MAX_SOURCE_BYTES,
                    ..ConversionBudgets::default()
                })
                .map_err(io::Error::other);
        }
        if started.elapsed() >= CHILD_DEADLINE {
            return Err(io::Error::other("PDF converter child exceeded deadline"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Reaps a conversion child before the stream's cancellation or deadline exits.
pub async fn convert_spool_in_worker(
    file: tokio::fs::File,
    executable: Option<&Path>,
    expected_sha256: &[u8],
    cancellation: &CancellationToken,
    deadline: tokio::time::Instant,
    disconnected: &CancellationToken,
) -> io::Result<PdfTextSource> {
    if expected_sha256.len() != 32 {
        return Err(io::Error::other(
            "large PDF stream requires a SHA-256 digest",
        ));
    }
    let digest = expected_sha256
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let executable =
        executable.map_or_else(std::env::current_exe, |path| Ok(path.to_path_buf()))?;
    let source = file.into_std().await;
    let child_cancellation = cancellation.child_token();
    let guard = child_cancellation.clone();
    let child_digest = digest.clone();
    let mut task = tokio::task::spawn_blocking(move || {
        convert_spool(&executable, &source, &child_digest, &guard)
    });
    let converted = tokio::select! {
        result = &mut task => result.map_err(io::Error::other)?,
        () = cancellation.cancelled() => {
            child_cancellation.cancel();
            let _ = task.await;
            return Err(io::Error::other("PDF conversion cancelled"));
        }
        () = disconnected.cancelled() => {
            child_cancellation.cancel();
            let _ = task.await;
            return Err(io::Error::other("PDF stream disconnected"));
        }
        () = tokio::time::sleep_until(deadline) => {
            child_cancellation.cancel();
            let _ = task.await;
            return Err(io::Error::other("PDF stream deadline exceeded"));
        }
    }?;
    Ok(PdfTextSource {
        content_hash: digest,
        converted,
    })
}

/// Converts a path-free stdin PDF and emits one validated, complete text result.
pub fn run(expected_sha256: &str) -> io::Result<()> {
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(io::Error::other("invalid PDF source digest"));
    }
    let mut source = Vec::new();
    io::stdin()
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut source)?;
    if source.len() as u64 > MAX_SOURCE_BYTES {
        return Err(io::Error::other("PDF source exceeds the child byte limit"));
    }
    let actual_digest = Sha256::digest(&source)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual_digest != expected_sha256 {
        return Err(io::Error::other(
            "PDF source digest does not match the stream",
        ));
    }
    let budgets = ConversionBudgets {
        max_source_bytes: MAX_SOURCE_BYTES,
        ..ConversionBudgets::default()
    };
    let total_pages = {
        let document = lopdf::Document::load_mem_with_options(
            &source,
            lopdf::LoadOptions::with_max_decompressed_size(
                fm_semantic_conversion::MAX_PDF_STRUCTURAL_STREAM_BYTES,
            ),
        )
        .map_err(io::Error::other)?;
        u32::try_from(
            fm_semantic_conversion::checked_pdf_pages(&document)
                .map_err(io::Error::other)?
                .len(),
        )
        .map_err(io::Error::other)?
    };
    if total_pages > budgets.max_items {
        return Err(io::Error::other(
            "PDF page count exceeds the conversion budget",
        ));
    }
    let metadata = DocumentMetadata::unknown().with_media_type("application/pdf");
    let mut windows = Vec::new();
    for start in (1..=total_pages).step_by(32) {
        let window =
            PdfPageWindow::new(start, (start + 31).min(total_pages)).map_err(io::Error::other)?;
        let outcome = BaselineConverter::new()
            .convert(
                SourceContent::Bytes(&source),
                &metadata,
                &ConversionContext::new()
                    .with_budgets(budgets.clone())
                    .with_pdf_page_window(window),
            )
            .map_err(io::Error::other)?;
        let ConversionOutcome::Converted(document) = outcome else {
            return Err(io::Error::other(format!(
                "PDF text conversion failed on page {start}: {outcome:?}"
            )));
        };
        windows.push((window, document));
    }
    let document =
        merge_pdf_text_windows(total_pages, windows, &budgets).map_err(io::Error::other)?;
    let report =
        PdfTextReport::from_document(total_pages, &document, &budgets).map_err(io::Error::other)?;
    let payload = serde_json::to_vec(&report).map_err(io::Error::other)?;
    if payload.len() > MAX_REPORT_BYTES {
        return Err(io::Error::other(
            "PDF text report exceeds the child output limit",
        ));
    }
    io::stdout().write_all(&payload)
}
