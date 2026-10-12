//! Opt-in, text-only converter child for bounded PDF source streams.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, MAX_VISUAL_BYTES, MAX_VISUAL_PIXELS, MediaType, PdfPageWindow, PdfTextReport,
    SourceContent, VisualAttachment, VisualProvenance, merge_pdf_text_windows,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio_util::sync::CancellationToken;

use crate::ingestion::PdfTextSource;

const MAX_SOURCE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_REPORT_BYTES: usize = MAX_VISUAL_BYTES + 4096;
const MAX_ERROR_BYTES: u64 = 16 * 1024;
// Experimental ceiling pending four-target installed qualification.
const MAX_SAMPLED_RSS: u64 = 512 * 1024 * 1024;
const CHILD_DEADLINE: Duration = Duration::from_secs(120);

struct KillOnDrop(Child);

fn read_child_output(
    reader: impl Read,
    maximum_output_bytes: usize,
    exceeded: mpsc::Sender<()>,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(maximum_output_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum_output_bytes {
        let _ = exceeded.send(());
    }
    Ok(bytes)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageImageHeader {
    page_number: u32,
    image_index: u32,
    media_type: String,
    width: u32,
    height: u32,
    id: String,
    byte_length: usize,
}

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
    let bytes = run_spool_child(
        executable,
        spool,
        expected_sha256,
        &["--pdf-text-child"],
        MAX_REPORT_BYTES,
        cancellation,
    )?;
    let report: PdfTextReport = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("PDF converter output was invalid"))?;
    report
        .into_document(&ConversionBudgets {
            max_source_bytes: MAX_SOURCE_BYTES,
            ..ConversionBudgets::default()
        })
        .map_err(io::Error::other)
}

/// Extracts one bounded image from one verified page in the killable child.
///
/// This is not a publication path; multi-image pages and unsupported encodings
/// fail explicitly until a complete page-coverage pipeline can account for them.
pub fn convert_spool_visual_page(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    page_number: u32,
    cancellation: &CancellationToken,
) -> io::Result<VisualAttachment> {
    if page_number == 0 {
        return Err(io::Error::other("PDF page number must be positive"));
    }
    let page = page_number.to_string();
    let mut bytes = run_spool_child(
        executable,
        spool,
        expected_sha256,
        &["--pdf-visual-child", &page],
        MAX_IMAGE_REPORT_BYTES,
        cancellation,
    )?;
    let header_length = bytes
        .get(..4)
        .and_then(|prefix| <[u8; 4]>::try_from(prefix).ok())
        .map(u32::from_be_bytes)
        .ok_or_else(|| io::Error::other("PDF image report has no header length"))?
        as usize;
    if header_length == 0 || header_length > 4092 || bytes.len() < 4 + header_length {
        return Err(io::Error::other("PDF image report header is invalid"));
    }
    let header: PageImageHeader = serde_json::from_slice(&bytes[4..4 + header_length])
        .map_err(|_| io::Error::other("PDF image report metadata is invalid"))?;
    let data = bytes.split_off(4 + header_length);
    if header.page_number != page_number
        || header.byte_length == 0
        || header.byte_length > MAX_VISUAL_BYTES
        || data.len() != header.byte_length
        || header.width == 0
        || header.height == 0
        || u64::from(header.width) * u64::from(header.height) > MAX_VISUAL_PIXELS
        || !matches!(header.media_type.as_str(), "image/jpeg" | "image/png")
        || blake3::hash(&data).to_hex().as_str() != header.id
    {
        return Err(io::Error::other("PDF image report failed validation"));
    }
    Ok(VisualAttachment {
        id: header.id,
        media_type: MediaType::parse(&header.media_type),
        data,
        width: header.width,
        height: header.height,
        provenance: VisualProvenance::PdfImage {
            page_number,
            image_index: header.image_index,
        },
        caption: None,
    })
}

fn run_spool_child(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    arguments: &[&str],
    maximum_output_bytes: usize,
    cancellation: &CancellationToken,
) -> io::Result<Vec<u8>> {
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
            .args(arguments)
            .arg(expected_sha256)
            .stdin(input)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing child output"))?;
    let (overflow_tx, overflow_rx) = mpsc::channel();
    let output =
        thread::spawn(move || read_child_output(stdout, maximum_output_bytes, overflow_tx));
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
        if overflow_rx.try_recv().is_ok() {
            return Err(io::Error::other(
                "PDF converter output exceeded its byte limit",
            ));
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
            if bytes.len() > maximum_output_bytes {
                return Err(io::Error::other(
                    "PDF converter output exceeded its byte limit",
                ));
            }
            return Ok(bytes);
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

fn read_verified_source(expected_sha256: &str) -> io::Result<Vec<u8>> {
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
    Ok(source)
}

/// Converts a path-free stdin PDF and emits one validated, complete text result.
pub fn run(expected_sha256: &str) -> io::Result<()> {
    let source = read_verified_source(expected_sha256)?;
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

/// Extracts one drawn raster from an authenticated PDF page, never a partial page.
pub fn run_visual_page(expected_sha256: &str, page_number: u32) -> io::Result<()> {
    if page_number == 0 {
        return Err(io::Error::other("PDF page number must be positive"));
    }
    let source = read_verified_source(expected_sha256)?;
    let window = PdfPageWindow::new(page_number, page_number).map_err(io::Error::other)?;
    let outcome = BaselineConverter::new()
        .convert(
            SourceContent::Bytes(&source),
            &DocumentMetadata::unknown().with_media_type("application/pdf"),
            &ConversionContext::new()
                .with_budgets(ConversionBudgets {
                    max_source_bytes: MAX_SOURCE_BYTES,
                    ..ConversionBudgets::default()
                })
                .with_pdf_page_window(window)
                .with_visual_evidence(true),
        )
        .map_err(io::Error::other)?;
    let ConversionOutcome::Converted(document) = outcome else {
        return Err(io::Error::other(format!(
            "PDF page {page_number} has no usable visual evidence: {outcome:?}"
        )));
    };
    if document.visuals().len() != 1 || !document.visual_omissions().is_empty() {
        return Err(io::Error::other(format!(
            "PDF page {page_number} has missing or multiple image candidates"
        )));
    }
    let visual = &document.visuals()[0];
    let VisualProvenance::PdfImage {
        page_number: cited_page,
        image_index,
    } = visual.provenance
    else {
        return Err(io::Error::other("PDF image has no page citation"));
    };
    if cited_page != page_number {
        return Err(io::Error::other("PDF image cites another page"));
    }
    let header = PageImageHeader {
        page_number,
        image_index,
        media_type: visual.media_type.as_str().to_owned(),
        width: visual.width,
        height: visual.height,
        id: visual.id.clone(),
        byte_length: visual.data.len(),
    };
    let encoded = serde_json::to_vec(&header).map_err(io::Error::other)?;
    if encoded.len() > 4092 || visual.data.len() > MAX_VISUAL_BYTES {
        return Err(io::Error::other("PDF image report exceeds its byte limit"));
    }
    let mut output = io::stdout().lock();
    output.write_all(
        &u32::try_from(encoded.len())
            .map_err(io::Error::other)?
            .to_be_bytes(),
    )?;
    output.write_all(&encoded)?;
    output.write_all(&visual.data)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use sha2::Digest as _;

    use super::read_child_output;

    struct NeverEndsAfterLimit {
        remaining: usize,
        release: mpsc::Receiver<()>,
    }

    impl Read for NeverEndsAfterLimit {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.remaining > 0 {
                let count = buffer.len().min(self.remaining);
                buffer[..count].fill(b'X');
                self.remaining -= count;
                return Ok(count);
            }
            self.release
                .recv()
                .map_err(|_| io::Error::other("test reader was not released"))?;
            Ok(0)
        }
    }

    #[test]
    fn oversized_child_output_signals_without_waiting_for_eof() {
        let (overflow_tx, overflow_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let reader = NeverEndsAfterLimit {
            remaining: 65,
            release: release_rx,
        };
        let task = std::thread::spawn(move || read_child_output(reader, 64, overflow_tx));
        let result = overflow_rx.recv_timeout(Duration::from_secs(1));
        let _ = release_tx.send(());
        assert_eq!(result, Ok(()));
        assert_eq!(
            task.join().expect("reader thread").expect("reader"),
            vec![b'X'; 65]
        );
    }

    #[cfg(unix)]
    #[test]
    fn oversized_child_is_stopped_before_its_deadline() {
        let mut spool = tempfile::tempfile().expect("anonymous spool");
        spool.write_all(b"synthetic").expect("write spool");
        let digest = super::Sha256::digest(b"synthetic")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let started = Instant::now();
        let error = super::run_spool_child(
            std::path::Path::new("/bin/sh"),
            &spool,
            &digest,
            &["-c", "while :; do printf '12345678901234567890'; done"],
            1024,
            &tokio_util::sync::CancellationToken::new(),
        )
        .expect_err("oversized output must stop the child");
        assert!(
            error.to_string().contains("output exceeded its byte limit"),
            "unexpected child error: {error}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
