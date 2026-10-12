//! Opt-in converter child for bounded PDF text and page-image streams.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, MAX_TOTAL_VISUAL_BYTES, MAX_VISUAL_ATTACHMENTS, MAX_VISUAL_BYTES,
    MAX_VISUAL_PIXELS, MediaType, PdfPageWindow, PdfTextReport, SourceContent, VisualAttachment,
    VisualProvenance, merge_pdf_text_windows,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio_util::sync::CancellationToken;

use crate::ingestion::PdfTextSource;

const MAX_SOURCE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_REPORT_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_REPORT_BYTES: usize = MAX_VISUAL_BYTES + 4096;
// Provisional experimental disk ceiling; installed four-target evidence is still required.
const MAX_STAGED_VISUAL_BYTES: usize = 256 * 1024 * 1024;
const MAX_ERROR_BYTES: u64 = 16 * 1024;
const PAGE_ERROR_PREFIX: &str = "PROCYON_PDF_PAGE_ERROR:";
// Experimental ceiling pending four-target installed qualification.
const MAX_SAMPLED_RSS: u64 = 512 * 1024 * 1024;
const CHILD_DEADLINE: Duration = Duration::from_secs(120);

struct KillOnDrop(Child);

fn read_child_output<S: Write>(
    reader: impl Read,
    maximum_output_bytes: usize,
    failure: mpsc::Sender<io::Error>,
    mut sink: S,
) -> io::Result<(S, u64)> {
    let copied = match io::copy(&mut reader.take(maximum_output_bytes as u64 + 1), &mut sink) {
        Ok(copied) => copied,
        Err(error) => {
            let _ = failure.send(io::Error::new(error.kind(), error.to_string()));
            return Err(error);
        }
    };
    if copied > maximum_output_bytes as u64 {
        let _ = failure.send(io::Error::other(
            "PDF converter output exceeded its byte limit",
        ));
    }
    Ok((sink, copied))
}

fn page_failure_from_stderr(stderr: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stderr).ok()?;
    for line in text.lines() {
        let Some((page, reason)) = line
            .strip_prefix(PAGE_ERROR_PREFIX)
            .and_then(|value| value.split_once(':'))
        else {
            continue;
        };
        let page = page.parse::<u32>().ok()?;
        if !(1..=ConversionBudgets::default().max_items).contains(&page)
            || !matches!(
                reason,
                "no usable visual evidence"
                    | "missing or multiple image candidates"
                    | "unsupported inline image"
                    | "renderer unavailable"
                    | "renderer geometry exceeds budget"
                    | "page conversion failed"
            )
        {
            return None;
        }
        return Some(format!("PDF page {page}: {reason}"));
    }
    None
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageImageHeader {
    page_number: u32,
    image_index: u32,
    #[serde(default)]
    rendered_page: bool,
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

/// Converts text only after every visual page is staged for this same source.
pub fn convert_spool_with_visual_stage(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    stage: &StagedPdfVisualPages,
    cancellation: &CancellationToken,
) -> io::Result<fm_semantic_conversion::ConvertedDocument> {
    if stage.source_digest() != expected_sha256 {
        return Err(io::Error::other("PDF text and image source digests differ"));
    }
    let bytes = run_spool_child(
        executable,
        spool,
        expected_sha256,
        &["--pdf-text-with-visuals-child"],
        MAX_REPORT_BYTES,
        cancellation,
    )?;
    let report: PdfTextReport = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("PDF converter output was invalid"))?;
    if report.total_pages() != stage.page_count() {
        return Err(io::Error::other("PDF text and image page counts differ"));
    }
    report
        .into_document_with_visual_stage(&ConversionBudgets {
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
    let bytes = run_spool_child(
        executable,
        spool,
        expected_sha256,
        &["--pdf-visual-child", &page],
        MAX_IMAGE_REPORT_BYTES,
        cancellation,
    )?;
    let mut report = io::Cursor::new(bytes);
    let image = read_image_report(&mut report, page_number)?
        .ok_or_else(|| io::Error::other("PDF page has no image"))?;
    if report.position() != report.get_ref().len() as u64 {
        return Err(io::Error::other("PDF image report contains trailing data"));
    }
    Ok(image)
}

/// Bounded anonymous staging of images per validated PDF page.
///
/// The stage is not a published generation and is deleted when its file closes.
#[derive(Debug)]
pub struct StagedPdfVisualPages {
    file: File,
    page_count: u32,
    next_page: u32,
    source_digest: String,
}

impl StagedPdfVisualPages {
    /// Number of validated pages in this staged PDF.
    #[must_use]
    pub const fn page_count(&self) -> u32 {
        self.page_count
    }

    /// Digest of the original source verified independently by both processes.
    #[must_use]
    pub fn source_digest(&self) -> &str {
        &self.source_digest
    }

    /// Temporary bytes retained until this anonymous stage is dropped.
    pub fn staged_bytes(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    /// Rewinds an already validated stage for an idempotent ingestion retry.
    pub fn rewind(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(4))?;
        self.next_page = 1;
        Ok(())
    }

    /// Reads one bounded page at a time; an empty list means text but no images.
    pub fn next_page(&mut self) -> io::Result<Option<Vec<VisualAttachment>>> {
        if self.next_page > self.page_count {
            return Ok(None);
        }
        let mut count = [0_u8; 4];
        self.file.read_exact(&mut count)?;
        let count = u32::from_be_bytes(count) as usize;
        if count > MAX_VISUAL_ATTACHMENTS {
            return Err(io::Error::other("PDF page exceeds image count limit"));
        }
        let mut images = Vec::with_capacity(count);
        let mut indices = std::collections::HashSet::with_capacity(count);
        let mut retained_bytes = 0_usize;
        for _ in 0..count {
            let image = read_image_report(&mut self.file, self.next_page)?
                .ok_or_else(|| io::Error::other("PDF page has an empty image record"))?;
            let image_index = match image.provenance {
                VisualProvenance::PdfImage { image_index, .. } => image_index,
                VisualProvenance::PdfPageRender { .. } => 0,
                _ => return Err(io::Error::other("PDF image lacks page provenance")),
            };
            if !indices.insert(image_index) {
                return Err(io::Error::other("PDF page repeats an image citation"));
            }
            retained_bytes = retained_bytes.saturating_add(image.data.len());
            if retained_bytes > MAX_TOTAL_VISUAL_BYTES {
                return Err(io::Error::other("PDF page exceeds image byte limit"));
            }
            images.push(image);
        }
        self.next_page += 1;
        Ok(Some(images))
    }
}

/// Runs one supervised child across all pages, staging rather than retaining images.
pub fn stage_spool_visual_pages(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    cancellation: &CancellationToken,
) -> io::Result<StagedPdfVisualPages> {
    let file = tempfile::tempfile()?;
    let file = run_spool_child_with_output(
        executable,
        spool,
        expected_sha256,
        &["--pdf-visual-pages-child"],
        MAX_STAGED_VISUAL_BYTES,
        cancellation,
        file,
    )?;
    validated_visual_stage(file, expected_sha256)
}

fn validated_visual_stage(
    mut file: File,
    expected_sha256: &str,
) -> io::Result<StagedPdfVisualPages> {
    file.seek(SeekFrom::Start(0))?;
    let mut count = [0_u8; 4];
    file.read_exact(&mut count)?;
    let page_count = u32::from_be_bytes(count);
    if page_count == 0 || page_count > ConversionBudgets::default().max_items {
        return Err(io::Error::other("PDF visual stage has invalid page count"));
    }
    let mut staged = StagedPdfVisualPages {
        file,
        page_count,
        next_page: 1,
        source_digest: expected_sha256.to_owned(),
    };
    while staged.next_page()?.is_some() {}
    if staged.file.read(&mut [0_u8; 1])? != 0 {
        return Err(io::Error::other("PDF visual stage contains trailing data"));
    }
    staged.file.seek(SeekFrom::Start(4))?;
    staged.next_page = 1;
    Ok(staged)
}

#[cfg(test)]
pub(crate) fn staged_visual_fixture(
    source_digest: &str,
    pages: &[Vec<VisualAttachment>],
) -> io::Result<StagedPdfVisualPages> {
    let mut file = tempfile::tempfile()?;
    let count = u32::try_from(pages.len()).map_err(io::Error::other)?;
    file.write_all(&count.to_be_bytes())?;
    for (index, images) in pages.iter().enumerate() {
        file.write_all(
            &u32::try_from(images.len())
                .map_err(io::Error::other)?
                .to_be_bytes(),
        )?;
        for image in images {
            let (image_index, rendered_page) = match image.provenance {
                VisualProvenance::PdfImage { image_index, .. } => (image_index, false),
                VisualProvenance::PdfPageRender { .. } => (0, true),
                _ => return Err(io::Error::other("fixture image lacks PDF provenance")),
            };
            let header = PageImageHeader {
                page_number: u32::try_from(index + 1).map_err(io::Error::other)?,
                image_index,
                rendered_page,
                media_type: image.media_type.as_str().into(),
                width: image.width,
                height: image.height,
                id: image.id.clone(),
                byte_length: image.data.len(),
            };
            write_image_report(&mut file, &header, &image.data)?;
        }
    }
    validated_visual_stage(file, source_digest)
}

fn read_image_report(
    reader: &mut impl Read,
    page_number: u32,
) -> io::Result<Option<VisualAttachment>> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let header_length = u32::from_be_bytes(length) as usize;
    if header_length == 0 {
        return Ok(None);
    }
    if header_length > 4092 {
        return Err(io::Error::other("PDF image report header is invalid"));
    }
    let mut encoded = vec![0_u8; header_length];
    reader.read_exact(&mut encoded)?;
    let header: PageImageHeader = serde_json::from_slice(&encoded)
        .map_err(|_| io::Error::other("PDF image report metadata is invalid"))?;
    if header.byte_length == 0 || header.byte_length > MAX_VISUAL_BYTES {
        return Err(io::Error::other("PDF image report byte length is invalid"));
    }
    let mut data = vec![0_u8; header.byte_length];
    reader.read_exact(&mut data)?;
    validate_image(header, data, page_number).map(Some)
}

fn validate_image(
    header: PageImageHeader,
    data: Vec<u8>,
    page_number: u32,
) -> io::Result<VisualAttachment> {
    if header.page_number != page_number
        || header.image_index == u32::MAX
        || (header.rendered_page && header.image_index != 0)
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
    let provenance = if header.rendered_page {
        VisualProvenance::PdfPageRender { page_number }
    } else {
        VisualProvenance::PdfImage {
            page_number,
            image_index: header.image_index,
        }
    };
    Ok(VisualAttachment {
        id: header.id,
        media_type: MediaType::parse(&header.media_type),
        data,
        width: header.width,
        height: header.height,
        provenance,
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
    run_spool_child_with_output(
        executable,
        spool,
        expected_sha256,
        arguments,
        maximum_output_bytes,
        cancellation,
        Vec::new(),
    )
}

fn run_spool_child_with_output<S: Write + Send + 'static>(
    executable: &Path,
    spool: &File,
    expected_sha256: &str,
    arguments: &[&str],
    maximum_output_bytes: usize,
    cancellation: &CancellationToken,
    sink: S,
) -> io::Result<S> {
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
    let (failure_tx, failure_rx) = mpsc::channel();
    let output =
        thread::spawn(move || read_child_output(stdout, maximum_output_bytes, failure_tx, sink));
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
        if let Ok(error) = failure_rx.try_recv() {
            return Err(error);
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
                let stderr = errors
                    .join()
                    .map_err(|_| io::Error::other("PDF converter error reader failed"))??;
                return Err(io::Error::other(
                    page_failure_from_stderr(&stderr)
                        .unwrap_or_else(|| "PDF converter child failed".into()),
                ));
            }
            if !sampled {
                return Err(io::Error::other("PDF converter had no memory sample"));
            }
            let result = output
                .join()
                .map_err(|_| io::Error::other("PDF converter output reader failed"))?;
            let (sink, count) = result.map_err(|error| failure_rx.try_recv().unwrap_or(error))?;
            errors
                .join()
                .map_err(|_| io::Error::other("PDF converter error reader failed"))??;
            if count > maximum_output_bytes as u64 {
                return Err(io::Error::other(
                    "PDF converter output exceeded its byte limit",
                ));
            }
            return Ok(sink);
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
    pdf_images_consented: bool,
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
        if pdf_images_consented {
            let stage = stage_spool_visual_pages(&executable, &source, &child_digest, &guard)?;
            let converted = convert_spool_with_visual_stage(
                &executable,
                &source,
                &child_digest,
                &stage,
                &guard,
            )?;
            Ok((converted, Some(stage)))
        } else {
            convert_spool(&executable, &source, &child_digest, &guard)
                .map(|converted| (converted, None))
        }
    });
    let (converted, visual_stage) = tokio::select! {
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
        visual_stage: visual_stage.map(|stage| std::sync::Arc::new(std::sync::Mutex::new(stage))),
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
    run_text(expected_sha256, false)
}

/// Emits text that may be empty only for the separately staged visual source.
pub fn run_with_visual_stage(expected_sha256: &str) -> io::Result<()> {
    run_text(expected_sha256, true)
}

fn run_text(expected_sha256: &str, allow_empty: bool) -> io::Result<()> {
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
        let document = match outcome {
            ConversionOutcome::Converted(document) => document,
            ConversionOutcome::NoTextLayer { .. } if allow_empty => {
                fm_semantic_conversion::ConvertedDocument::new(
                    fm_semantic_conversion::BASELINE_CONVERTER_VERSION,
                    fm_semantic_conversion::FormatKind::Pdf,
                    Vec::new(),
                    Vec::new(),
                    if window.end() - window.start() + 1 == total_pages {
                        Vec::new()
                    } else {
                        vec![fm_semantic_conversion::Omission::ItemsDropped {
                            item: "page",
                            converted: window.end() - window.start() + 1,
                            total: total_pages,
                        }]
                    },
                )
            }
            _ => {
                return Err(io::Error::other(format!(
                    "PDF text conversion failed on page {start}: {outcome:?}"
                )));
            }
        };
        windows.push((window, document));
    }
    let document =
        merge_pdf_text_windows(total_pages, windows, &budgets).map_err(io::Error::other)?;
    let report = if allow_empty {
        PdfTextReport::from_document_with_visual_stage(total_pages, &document, &budgets)
    } else {
        PdfTextReport::from_document(total_pages, &document, &budgets)
    }
    .map_err(io::Error::other)?;
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
    write_visual_page(&source, page_number, &mut io::stdout().lock(), false, 0)
}

/// Writes complete page-cited image reports, one at a time, without retaining
/// the previous pages' encoded bytes.
pub fn run_visual_pages(expected_sha256: &str) -> io::Result<()> {
    let source = read_verified_source(expected_sha256)?;
    let document = lopdf::Document::load_mem_with_options(
        &source,
        lopdf::LoadOptions::with_max_decompressed_size(
            fm_semantic_conversion::MAX_PDF_STRUCTURAL_STREAM_BYTES,
        ),
    )
    .map_err(io::Error::other)?;
    let count = u32::try_from(
        fm_semantic_conversion::checked_pdf_pages(&document)
            .map_err(io::Error::other)?
            .len(),
    )
    .map_err(io::Error::other)?;
    if count == 0 || count > ConversionBudgets::default().max_items {
        return Err(io::Error::other("PDF page count exceeds the visual budget"));
    }
    drop(document);
    let mut output = io::stdout().lock();
    output.write_all(&count.to_be_bytes())?;
    for page in 1..=count {
        if let Err(error) = write_visual_page(&source, page, &mut output, true, count) {
            let reason = if error.to_string().contains("has no usable visual evidence") {
                "no usable visual evidence"
            } else if error
                .to_string()
                .contains("missing or multiple image candidates")
            {
                "missing or multiple image candidates"
            } else if error.to_string().contains("has unsupported inline image") {
                "unsupported inline image"
            } else if error.to_string().contains("PDF renderer unavailable") {
                "renderer unavailable"
            } else if error
                .to_string()
                .contains("PDF renderer geometry exceeds the rendering budget")
            {
                "renderer geometry exceeds budget"
            } else {
                "page conversion failed"
            };
            writeln!(io::stderr(), "{PAGE_ERROR_PREFIX}{page}:{reason}")?;
            return Err(io::Error::other(format!("PDF page {page}: {reason}")));
        }
    }
    Ok(())
}

fn write_visual_page(
    source: &[u8],
    page_number: u32,
    output: &mut impl Write,
    staged: bool,
    _expected_page_count: u32,
) -> io::Result<()> {
    let window = PdfPageWindow::new(page_number, page_number).map_err(io::Error::other)?;
    let outcome = BaselineConverter::new()
        .convert(
            SourceContent::Bytes(source),
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
    #[cfg(feature = "experimental-pdf-render")]
    if staged
        && match &outcome {
            ConversionOutcome::NoTextLayer { .. } => true,
            ConversionOutcome::Converted(document) => {
                document.visuals().is_empty() && !document.visual_omissions().is_empty()
            }
            _ => false,
        }
    {
        let visual =
            crate::pdf_render_child::render_page(source, page_number, _expected_page_count)?;
        let header = PageImageHeader {
            page_number,
            image_index: 0,
            rendered_page: true,
            media_type: visual.media_type.as_str().to_owned(),
            width: visual.width,
            height: visual.height,
            id: visual.id,
            byte_length: visual.data.len(),
        };
        output.write_all(&1_u32.to_be_bytes())?;
        return write_image_report(output, &header, &visual.data);
    }
    let ConversionOutcome::Converted(document) = outcome else {
        return Err(io::Error::other(format!(
            "PDF page {page_number} has no usable visual evidence: {outcome:?}"
        )));
    };
    if document
        .visual_omissions()
        .iter()
        .any(|omission| omission.detail.contains("inline image"))
    {
        return Err(io::Error::other(format!(
            "PDF page {page_number} has unsupported inline image"
        )));
    }
    if !document.visual_omissions().is_empty()
        || (document.visuals().is_empty() && document.units().is_empty())
        || (!staged && document.visuals().len() > 1)
    {
        return Err(io::Error::other(format!(
            "PDF page {page_number} has missing or multiple image candidates"
        )));
    }
    if staged {
        output.write_all(
            &u32::try_from(document.visuals().len())
                .map_err(io::Error::other)?
                .to_be_bytes(),
        )?;
    }
    if !staged && document.visuals().is_empty() {
        return output.write_all(&0_u32.to_be_bytes());
    }
    for visual in document.visuals() {
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
            rendered_page: false,
            media_type: visual.media_type.as_str().to_owned(),
            width: visual.width,
            height: visual.height,
            id: visual.id.clone(),
            byte_length: visual.data.len(),
        };
        write_image_report(output, &header, &visual.data)?;
    }
    Ok(())
}

fn write_image_report(
    output: &mut impl Write,
    header: &PageImageHeader,
    data: &[u8],
) -> io::Result<()> {
    let encoded = serde_json::to_vec(&header).map_err(io::Error::other)?;
    if encoded.len() > 4092 || data.len() > MAX_VISUAL_BYTES {
        return Err(io::Error::other("PDF image report exceeds its byte limit"));
    }
    output.write_all(
        &u32::try_from(encoded.len())
            .map_err(io::Error::other)?
            .to_be_bytes(),
    )?;
    output.write_all(&encoded)?;
    output.write_all(data)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use sha2::Digest as _;

    use super::{PageImageHeader, read_child_output, staged_visual_fixture, validate_image};

    #[test]
    fn staged_page_rejects_duplicate_citations_and_excess_images() {
        let data = vec![0xff, 0xd8, 0xff, 0xd9];
        let image = fm_semantic_conversion::VisualAttachment {
            id: blake3::hash(&data).to_hex().to_string(),
            media_type: fm_semantic_conversion::MediaType::parse("image/jpeg"),
            data,
            width: 2,
            height: 1,
            provenance: fm_semantic_conversion::VisualProvenance::PdfImage {
                page_number: 1,
                image_index: 0,
            },
            caption: None,
        };
        assert!(
            staged_visual_fixture("synthetic source", &[vec![image.clone(), image.clone()]])
                .expect_err("duplicate image citation")
                .to_string()
                .contains("repeats an image citation")
        );
        assert!(
            staged_visual_fixture(
                "synthetic source",
                &[vec![
                    image;
                    fm_semantic_conversion::MAX_VISUAL_ATTACHMENTS + 1
                ]]
            )
            .expect_err("too many page images")
            .to_string()
            .contains("image count limit")
        );
        let data = vec![0xff, 0xd8, 0xff, 0xd9];
        let header = PageImageHeader {
            page_number: 1,
            image_index: u32::MAX,
            rendered_page: false,
            media_type: "image/jpeg".into(),
            width: 2,
            height: 1,
            id: blake3::hash(&data).to_hex().to_string(),
            byte_length: data.len(),
        };
        assert!(validate_image(header, data, 1).is_err());
    }

    #[test]
    fn page_error_diagnostics_accept_only_bounded_known_codes() {
        assert_eq!(
            super::page_failure_from_stderr(
                b"PROCYON_PDF_PAGE_ERROR:354:no usable visual evidence\n"
            ),
            Some("PDF page 354: no usable visual evidence".into())
        );
        assert_eq!(
            super::page_failure_from_stderr(b"PROCYON_PDF_PAGE_ERROR:2:unsupported inline image\n"),
            Some("PDF page 2: unsupported inline image".into())
        );
        assert_eq!(
            super::page_failure_from_stderr(
                b"PROCYON_PDF_PAGE_ERROR:2:renderer geometry exceeds budget\n"
            ),
            Some("PDF page 2: renderer geometry exceeds budget".into())
        );
        assert!(
            super::page_failure_from_stderr(b"PROCYON_PDF_PAGE_ERROR:354:secret source text")
                .is_none()
        );
        assert!(
            super::page_failure_from_stderr(
                b"PROCYON_PDF_PAGE_ERROR:999999:no usable visual evidence"
            )
            .is_none()
        );
    }

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
        let task =
            std::thread::spawn(move || read_child_output(reader, 64, overflow_tx, Vec::new()));
        let result = overflow_rx.recv_timeout(Duration::from_secs(1));
        let _ = release_tx.send(());
        assert!(
            result
                .expect("overflow signal")
                .to_string()
                .contains("output exceeded its byte limit")
        );
        assert_eq!(
            task.join().expect("reader thread").expect("reader"),
            (vec![b'X'; 65], 65)
        );
    }

    #[test]
    fn failed_stage_write_signals_without_waiting_for_child_exit() {
        #[derive(Debug)]
        struct FailingStage;
        impl Write for FailingStage {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("synthetic stage disk full"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let (failure_tx, failure_rx) = mpsc::channel();
        let result = read_child_output(io::Cursor::new([42_u8; 16]), 32, failure_tx, FailingStage);
        assert!(result.is_err());
        assert!(
            failure_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("prompt stage failure")
                .to_string()
                .contains("synthetic stage disk full")
        );
        #[cfg(unix)]
        {
            let mut spool = tempfile::tempfile().expect("anonymous spool");
            spool.write_all(b"synthetic").expect("write spool");
            let digest = super::Sha256::digest(b"synthetic")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let started = Instant::now();
            let error = super::run_spool_child_with_output(
                std::path::Path::new("/bin/sh"),
                &spool,
                &digest,
                &["-c", "while :; do printf '12345678901234567890'; done"],
                1024,
                &tokio_util::sync::CancellationToken::new(),
                FailingStage,
            )
            .expect_err("child must stop after stage write fails");
            assert!(error.to_string().contains("synthetic stage disk full"));
            assert!(started.elapsed() < Duration::from_secs(2));
        }
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
