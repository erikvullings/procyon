//! Optional `docling.rs` PDF conversion Adapter.
//!
//! This Module is deliberately separate from `fm-semantic-conversion`: ordinary
//! indexing keeps the pure-Rust baseline and does not link the optional
//! PDFium/ONNX implementation. The `ml` feature enables those native
//! dependencies for managed advanced-pack builds.

#[cfg(windows)]
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use docling_core::{DoclingDocument, FieldItem, Node, Table};
use docling_pdf::{PdfError, convert_text_layer_pages};
use fm_semantic_conversion::{
    AdvancedCapability, AdvancedConversion, AdvancedConverterAdapter, AdvancedConverterBackend,
    BaselineConverter, ComponentVersion, ConversionContext, ConversionError, ConversionOutcome,
    ConversionWarning, ConvertedDocument, DocumentConverter, DocumentMetadata, FormatKind,
    Omission, OptionalConverter, Provenance, ProvenancePrecision, SourceContent, SourceMap,
    StructuralUnit, TopLevelBoundary, UnitKind, instruction_like_excerpt, sanitize,
};

const OCR_REQUIRED_GUIDANCE: &str = "This PDF has no searchable text layer and was excluded from \
    semantic indexing. Add one with OCRmyPDF, then reindex the file. On macOS install OCRmyPDF \
    with Homebrew; on Linux install the distribution package; on Windows run it through WSL.";

/// Version of the Docling extraction and Procyon structural mapping behavior.
pub const DOCLING_PDF_CONVERTER_VERSION: ComponentVersion =
    ComponentVersion::new("docling-pdf", 1_036_000);
/// Stable identity of the production conversion pipeline.
///
/// This changes whenever the preferred converter or fallback behaviour can
/// produce different derived chunks for ordinary inputs. Optional OCR is not
/// included: enabling it retries only previously textless PDFs, while
/// disabling it makes reconciliation remove their OCR-derived occurrences.
/// OCR-derived documents carry [`OCRMYPDF_CONVERTER_VERSION`] themselves.
pub const DEFAULT_CONVERTER_PIPELINE_VERSION: &str = "docling-pdf/1036000+baseline/1";
/// Version of the optional OCRmyPDF plus deterministic Docling composition.
pub const OCRMYPDF_CONVERTER_VERSION: ComponentVersion =
    ComponentVersion::new("ocrmypdf-docling", 1);
/// Explicit environment opt-in read by the local semantic worker.
pub const OCRMYPDF_ENABLED_ENV: &str = "PROCYON_SEMANTIC_OCRMYPDF";
/// Optional trusted executable override for non-standard installations.
pub const OCRMYPDF_EXECUTABLE_ENV: &str = "PROCYON_OCRMYPDF_EXECUTABLE";
const DEFAULT_OCR_TIMEOUT: Duration = Duration::from_secs(4 * 60);
const OCR_POLL_INTERVAL: Duration = Duration::from_millis(25);
const OCRMYPDF_VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const OCRMYPDF_VERSION_OUTPUT_LIMIT: usize = 16 * 1024;
/// Oldest stable OCRmyPDF release audited for the converter's
/// `--output-type pdf --redo-ocr --optimize 0 --quiet` invocation.
pub const OCRMYPDF_MINIMUM_SUPPORTED_VERSION: &str = "16.0.0";
/// Exclusive upper bound for audited OCRmyPDF releases.
///
/// Stable 16.x and 17.x releases are accepted. Pre-releases and future major
/// versions require a compatibility review before this bound is raised.
pub const OCRMYPDF_MAXIMUM_SUPPORTED_VERSION_EXCLUSIVE: &str = "18.0.0";

/// Parsed OCRmyPDF semantic version reported by the resolved executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrMyPdfVersion {
    /// Major release number.
    pub major: u64,
    /// Minor release number.
    pub minor: u64,
    /// Patch release number.
    pub patch: u64,
    /// Optional pre-release identifier.
    pub pre_release: Option<String>,
}

impl OcrMyPdfVersion {
    /// Creates a stable OCRmyPDF version.
    #[must_use]
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            pre_release: None,
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let (core, pre_release) = match value.split_once('-') {
            Some((core, pre_release))
                if !pre_release.is_empty()
                    && pre_release.split('.').all(|part| {
                        !part.is_empty()
                            && part
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    }) =>
            {
                (core, Some(pre_release.to_owned()))
            }
            Some(_) => return None,
            None => (value, None),
        };
        let mut components = core.split('.');
        let major = components.next()?.parse().ok()?;
        let minor = components.next()?.parse().ok()?;
        let patch = components.next()?.parse().ok()?;
        if components.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre_release,
        })
    }
}

impl std::fmt::Display for OcrMyPdfVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre_release) = &self.pre_release {
            write!(formatter, "-{pre_release}")?;
        }
        Ok(())
    }
}

/// OCR language bundled by the audited Docling release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrLanguage {
    /// English recognition model.
    English,
    /// Docling's multilingual Chinese/Latin recognition model.
    Multilingual,
}

impl OcrLanguage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Multilingual => "ch",
        }
    }
}

/// Failure to initialize the managed ML pipeline.
#[cfg(feature = "ml")]
#[derive(Debug)]
pub struct DoclingInitError(String);

#[cfg(feature = "ml")]
impl std::fmt::Display for DoclingInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(feature = "ml")]
impl std::error::Error for DoclingInitError {}

/// Docling PDF backend with deterministic and managed-ML modes.
pub struct DoclingPdfBackend {
    #[cfg(feature = "ml")]
    pipeline: Option<std::sync::Mutex<docling_pdf::Pipeline>>,
    #[cfg(feature = "ml")]
    ocr_language: Option<OcrLanguage>,
}

impl std::fmt::Debug for DoclingPdfBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("DoclingPdfBackend");
        #[cfg(feature = "ml")]
        debug.field("ml", &self.pipeline.is_some());
        debug.finish_non_exhaustive()
    }
}

impl Default for DoclingPdfBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl DoclingPdfBackend {
    /// Creates the deterministic backend.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            #[cfg(feature = "ml")]
            pipeline: None,
            #[cfg(feature = "ml")]
            ocr_language: None,
        }
    }

    /// Creates the layout/OCR/table backend used by a managed advanced pack.
    ///
    /// The caller must set Docling's model and PDFium paths to files from the
    /// verified pack before construction; this function performs no download.
    #[cfg(feature = "ml")]
    pub fn try_ml(language: OcrLanguage) -> Result<Self, DoclingInitError> {
        let mut missing = docling_pdf::model_inventory()
            .into_iter()
            .filter(|asset| asset.stage != "ocr.rec" && asset.stage != "ocr.dict")
            .filter(|asset| !asset.found)
            .map(|asset| format!("{} ({})", asset.stage, asset.path))
            .collect::<Vec<_>>();
        for (stage, path) in requested_ocr_assets(language) {
            if !std::path::Path::new(&path).is_file() {
                missing.push(format!("{stage} ({path})"));
            }
        }
        if !missing.is_empty() {
            return Err(DoclingInitError(format!(
                "the verified Docling pack is incomplete; missing {}",
                missing.join(", ")
            )));
        }
        let mut pipeline =
            docling_pdf::Pipeline::new().map_err(|error| DoclingInitError(error.to_string()))?;
        let upstream_language = match language {
            OcrLanguage::English => docling_pdf::OcrLang::En,
            OcrLanguage::Multilingual => docling_pdf::OcrLang::Ch,
        };
        pipeline.set_ocr_lang(Some(upstream_language));
        pipeline.set_heading_hierarchy(docling_pdf::HeadingHierarchyOptions::enabled(true));
        Ok(Self {
            pipeline: Some(std::sync::Mutex::new(pipeline)),
            ocr_language: Some(language),
        })
    }
}

/// Availability of a supported local OCRmyPDF capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrMyPdfAvailability {
    /// A local executable was found, safely probed, and is supported.
    Available {
        /// Canonical absolute executable selected from the sanitized `PATH` or
        /// a documented platform installation location.
        executable: PathBuf,
        /// Version reported by the resolved executable.
        version: OcrMyPdfVersion,
    },
    /// No safe, supported executable is available.
    Unavailable {
        /// Typed reason the discovered installation was rejected.
        reason: OcrMyPdfRejectionReason,
        /// Cross-platform installation and manual remediation guidance.
        guidance: String,
    },
}

/// Reason production OCRmyPDF discovery rejected an installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrMyPdfRejectionReason {
    /// No executable was present in a safe search location.
    Missing,
    /// A candidate exists but is not a regular executable file.
    NonExecutable {
        /// Absolute candidate path.
        path: PathBuf,
    },
    /// The executable could not be launched for its version probe.
    CouldNotExecute,
    /// The executable returned no valid OCRmyPDF semantic version.
    MalformedVersion,
    /// The installed version is outside Procyon's audited range.
    UnsupportedVersion {
        /// Installed version.
        version: OcrMyPdfVersion,
    },
    /// The fixed version probe exceeded its deadline.
    TimedOut,
    /// Version output exceeded the fixed capture limit.
    OutputTooLarge {
        /// Maximum combined stdout and stderr bytes.
        limit: usize,
    },
}

impl OcrMyPdfAvailability {
    /// Discovers and version-checks OCRmyPDF for production use.
    ///
    /// Only the literal `ocrmypdf` executable name is searched. Relative and
    /// empty `PATH` entries are ignored, candidates are canonicalized, and the
    /// selected regular executable is invoked only with `--version`. In
    /// addition to sanitized `PATH`, macOS checks `/opt/homebrew/bin` and
    /// `/usr/local/bin`; Linux checks `/usr/bin`, `/usr/local/bin`, and
    /// `/snap/bin`. Native Windows reports WSL installation guidance rather
    /// than launching an unbounded intermediary.
    #[must_use]
    pub fn discover() -> Self {
        discover_ocrmypdf_from_candidates(
            production_ocrmypdf_candidates(),
            OCRMYPDF_VERSION_TIMEOUT,
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        )
    }

    /// Builds converter configuration only from a production-discovered
    /// executable. Rejected discoveries never yield a runnable command.
    #[must_use]
    pub fn configuration(&self) -> Option<OcrMyPdfConfiguration> {
        match self {
            Self::Available { executable, .. } => {
                Some(OcrMyPdfConfiguration::new(executable.clone()))
            }
            Self::Unavailable { .. } => None,
        }
    }

    /// Detects a trusted development executable.
    ///
    /// This compatibility entry point accepts a host-provided path for local
    /// development. Production callers must use [`Self::discover`].
    #[must_use]
    pub fn detect(executable_override: Option<&Path>) -> Self {
        let executable = executable_override
            .map(Path::to_path_buf)
            .or_else(|| find_executable("ocrmypdf"));
        let availability = discover_ocrmypdf_from_candidates(
            executable,
            OCRMYPDF_VERSION_TIMEOUT,
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        );
        match availability {
            Self::Unavailable { reason, .. } => Self::Unavailable {
                reason,
                guidance: OCR_REQUIRED_GUIDANCE.into(),
            },
            available => available,
        }
    }
}

fn discover_ocrmypdf_from_candidates(
    candidates: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
    output_limit: usize,
) -> OcrMyPdfAvailability {
    let mut rejection = None;
    for candidate in candidates {
        if !candidate.is_absolute() {
            continue;
        }
        let absolute = match candidate.canonicalize() {
            Ok(path) if path.is_absolute() => path,
            Ok(_) | Err(_) => {
                if candidate.exists() {
                    rejection = Some(OcrMyPdfRejectionReason::NonExecutable { path: candidate });
                }
                continue;
            }
        };
        if !is_regular_executable(&absolute) {
            rejection = Some(OcrMyPdfRejectionReason::NonExecutable { path: absolute });
            continue;
        }
        match probe_ocrmypdf_version(&absolute, timeout, output_limit) {
            Ok(version) if is_supported_ocrmypdf_version(&version) => {
                return OcrMyPdfAvailability::Available {
                    executable: absolute,
                    version,
                };
            }
            Ok(version) => {
                rejection = Some(OcrMyPdfRejectionReason::UnsupportedVersion { version });
            }
            Err(reason) => rejection = Some(reason),
        }
    }
    OcrMyPdfAvailability::Unavailable {
        reason: rejection.unwrap_or(OcrMyPdfRejectionReason::Missing),
        guidance: ocrmypdf_installation_guidance().into(),
    }
}

fn probe_ocrmypdf_version(
    executable: &Path,
    timeout: Duration,
    output_limit: usize,
) -> Result<OcrMyPdfVersion, OcrMyPdfRejectionReason> {
    let mut command = Command::new(executable);
    command.arg("--version");
    configure_version_environment(&mut command, executable);
    configure_child_process_group(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| OcrMyPdfRejectionReason::CouldNotExecute)?;
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = spawn_limited_reader(
        child.stdout.take().expect("piped stdout"),
        output_limit / 2,
        Arc::clone(&exceeded),
    );
    let stderr = spawn_limited_reader(
        child.stderr.take().expect("piped stderr"),
        output_limit - output_limit / 2,
        Arc::clone(&exceeded),
    );
    let started = Instant::now();
    let status = loop {
        if exceeded.load(Ordering::Acquire) {
            terminate_child(&mut child);
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(OcrMyPdfRejectionReason::OutputTooLarge {
                limit: output_limit,
            });
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            let _ = stdout.join();
            let _ = stderr.join();
            return Err(OcrMyPdfRejectionReason::TimedOut);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(OCR_POLL_INTERVAL),
            Err(_) => {
                terminate_child(&mut child);
                let _ = stdout.join();
                let _ = stderr.join();
                return Err(OcrMyPdfRejectionReason::CouldNotExecute);
            }
        }
    };
    terminate_child(&mut child);
    let stdout = stdout
        .join()
        .map_err(|_| OcrMyPdfRejectionReason::CouldNotExecute)?
        .map_err(|_| OcrMyPdfRejectionReason::CouldNotExecute)?;
    let stderr = stderr
        .join()
        .map_err(|_| OcrMyPdfRejectionReason::CouldNotExecute)?
        .map_err(|_| OcrMyPdfRejectionReason::CouldNotExecute)?;
    if exceeded.load(Ordering::Acquire) {
        return Err(OcrMyPdfRejectionReason::OutputTooLarge {
            limit: output_limit,
        });
    }
    if !status.success() {
        return Err(OcrMyPdfRejectionReason::CouldNotExecute);
    }
    parse_ocrmypdf_version(if stdout.is_empty() { &stderr } else { &stdout })
        .ok_or(OcrMyPdfRejectionReason::MalformedVersion)
}

fn spawn_limited_reader(
    mut stream: impl Read + Send + 'static,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(256));
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                return Ok(captured);
            }
            let remaining = limit.saturating_sub(captured.len());
            captured.extend_from_slice(&buffer[..read.min(remaining)]);
            if read > remaining {
                exceeded.store(true, Ordering::Release);
            }
        }
    })
}

fn parse_ocrmypdf_version(output: &[u8]) -> Option<OcrMyPdfVersion> {
    let output = std::str::from_utf8(output).ok()?.trim();
    let version = output
        .strip_prefix("ocrmypdf ")
        .or_else(|| output.strip_prefix("OCRmyPDF "))
        .unwrap_or(output);
    OcrMyPdfVersion::parse(version.trim())
}

fn is_supported_ocrmypdf_version(version: &OcrMyPdfVersion) -> bool {
    let minimum =
        OcrMyPdfVersion::parse(OCRMYPDF_MINIMUM_SUPPORTED_VERSION).expect("valid minimum version");
    let maximum = OcrMyPdfVersion::parse(OCRMYPDF_MAXIMUM_SUPPORTED_VERSION_EXCLUSIVE)
        .expect("valid maximum version");
    let current = (version.major, version.minor, version.patch);
    version.pre_release.is_none()
        && current >= (minimum.major, minimum.minor, minimum.patch)
        && current < (maximum.major, maximum.minor, maximum.patch)
}

fn ocrmypdf_installation_guidance() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Install supported OCRmyPDF 16.x or 17.x with Homebrew (`brew install ocrmypdf`). \
         Procyon never downloads or installs it."
    }
    #[cfg(target_os = "linux")]
    {
        "Install supported OCRmyPDF 16.x or 17.x from your distribution package manager. \
         Procyon never downloads or installs it."
    }
    #[cfg(target_os = "windows")]
    {
        "Install supported OCRmyPDF 16.x or 17.x in WSL and run Procyon's local semantic \
         environment there. Procyon never downloads or installs it."
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "Install supported OCRmyPDF 16.x or 17.x using the official platform instructions. \
         Procyon never downloads or installs it."
    }
}

/// Trusted, explicit configuration for the local OCR subprocess.
#[derive(Debug, Clone)]
pub struct OcrMyPdfConfiguration {
    executable: PathBuf,
    timeout: Duration,
    temporary_root: Option<PathBuf>,
}

impl OcrMyPdfConfiguration {
    /// Selects a trusted executable with the production OCR deadline.
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            timeout: DEFAULT_OCR_TIMEOUT,
            temporary_root: None,
        }
    }

    /// Overrides the hard OCR subprocess deadline.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Selects a trusted parent for private temporary files.
    ///
    /// Production leaves this unset to use the platform temporary directory;
    /// tests use it to prove cleanup on every exit path.
    #[must_use]
    pub fn with_temporary_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.temporary_root = Some(root.into());
        self
    }

    /// Resolves the local executable only after the environment opt-in is set
    /// to exactly `1`.
    #[must_use]
    pub fn from_environment() -> Option<Self> {
        if std::env::var(OCRMYPDF_ENABLED_ENV).as_deref() != Ok("1") {
            return None;
        }
        let executable_override = std::env::var_os(OCRMYPDF_EXECUTABLE_ENV).map(PathBuf::from);
        executable_override
            .or_else(|| find_executable("ocrmypdf"))
            .filter(|executable| executable.is_file())
            .map(Self::new)
    }
}

/// OCRmyPDF fallback around the deterministic Docling-first composition.
///
/// The wrapped converter always gets the original bytes first. OCR is started
/// only for its typed `NoTextLayer` outcome, and its output is passed through
/// the same wrapped converter rather than trusted as extracted text.
pub struct OcrMyPdfConverter {
    deterministic: Arc<dyn DocumentConverter>,
    configuration: OcrMyPdfConfiguration,
}

impl std::fmt::Debug for OcrMyPdfConverter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OcrMyPdfConverter")
            .field("configuration", &self.configuration)
            .finish_non_exhaustive()
    }
}

impl OcrMyPdfConverter {
    /// Wraps a deterministic converter with explicit local OCR configuration.
    #[must_use]
    pub fn new(
        deterministic: Arc<dyn DocumentConverter>,
        configuration: OcrMyPdfConfiguration,
    ) -> Self {
        Self {
            deterministic,
            configuration,
        }
    }

    fn no_text(detail: impl Into<String>) -> ConversionOutcome {
        ConversionOutcome::NoTextLayer {
            detail: format!("{} {}", detail.into(), OCR_REQUIRED_GUIDANCE),
        }
    }

    fn run_ocr(
        &self,
        source: &[u8],
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<ConversionOutcome, ConversionError> {
        if context.is_cancelled() {
            return Ok(ConversionOutcome::Cancelled);
        }
        let remaining_conversion_time = context.budgets().timeout.saturating_sub(context.elapsed());
        if remaining_conversion_time.is_zero() {
            return Ok(ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::Time,
                limit: duration_millis_u64(context.budgets().timeout),
            });
        }
        let mut temporary_directory = tempfile::Builder::new();
        temporary_directory.prefix("procyon-ocr-");
        let temporary_directory = match self.configuration.temporary_root.as_deref() {
            Some(root) => temporary_directory.tempdir_in(root),
            None => temporary_directory.tempdir(),
        }
        .map_err(ConversionError::Read)?;
        let input_path = temporary_directory.path().join("input.pdf");
        let output_path = temporary_directory.path().join("output.pdf");
        fs::write(&input_path, source).map_err(ConversionError::Read)?;

        let mut command = Command::new(&self.configuration.executable);
        command
            .arg("--output-type")
            .arg("pdf")
            .arg("--redo-ocr")
            .arg("--optimize")
            .arg("0")
            .arg("--quiet")
            .arg(&input_path)
            .arg(&output_path);
        configure_child_environment(
            &mut command,
            temporary_directory.path(),
            &self.configuration.executable,
        );
        configure_child_process_group(&mut command);
        let mut child = match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return Ok(Self::no_text(format!(
                    "OCRmyPDF could not be started: {error}."
                )));
            }
        };

        let started = Instant::now();
        let reconversion_reserve = (remaining_conversion_time / 10).min(Duration::from_secs(30));
        let deadline = self
            .configuration
            .timeout
            .min(remaining_conversion_time.saturating_sub(reconversion_reserve));
        let status = loop {
            if context.is_cancelled() {
                terminate_child(&mut child);
                return Ok(ConversionOutcome::Cancelled);
            }
            if started.elapsed() >= deadline {
                terminate_child(&mut child);
                return Ok(Self::no_text(format!(
                    "OCRmyPDF exceeded its {} ms local deadline.",
                    duration_millis_u64(deadline)
                )));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => std::thread::sleep(OCR_POLL_INTERVAL),
                Err(error) => {
                    terminate_child(&mut child);
                    return Err(ConversionError::Read(error));
                }
            }
        };

        if !status.success() {
            return Ok(Self::no_text(format!(
                "OCRmyPDF exited unsuccessfully ({status})."
            )));
        }
        let output_length = match fs::metadata(&output_path) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                return Ok(Self::no_text(format!(
                    "OCRmyPDF did not produce a readable PDF: {error}."
                )));
            }
        };
        if output_length > context.budgets().max_source_bytes {
            return Ok(Self::no_text(format!(
                "OCRmyPDF output exceeded the {} byte conversion limit.",
                context.budgets().max_source_bytes
            )));
        }
        let output = fs::read(&output_path).map_err(ConversionError::Read)?;
        let outcome =
            self.deterministic
                .convert(SourceContent::Bytes(&output), metadata, context)?;
        let outcome = match outcome {
            ConversionOutcome::NoTextLayer { .. } => BaselineConverter::new().convert(
                SourceContent::Bytes(&output),
                metadata,
                context,
            )?,
            other => other,
        };
        Ok(match outcome {
            ConversionOutcome::Converted(document) => {
                let mut warnings = document.warnings().to_vec();
                warnings.push(ConversionWarning::OcrAssessment {
                    language: "ocrmypdf-auto".into(),
                    mean_confidence_basis_points: None,
                });
                ConversionOutcome::Converted(ConvertedDocument::new(
                    OCRMYPDF_CONVERTER_VERSION,
                    document.format(),
                    document.units().to_vec(),
                    warnings,
                    document.omissions().to_vec(),
                ))
            }
            ConversionOutcome::NoTextLayer { .. } => Self::no_text(
                "OCRmyPDF completed, but deterministic conversion still found no searchable text.",
            ),
            other => other,
        })
    }
}

impl DocumentConverter for OcrMyPdfConverter {
    fn version(&self) -> ComponentVersion {
        OCRMYPDF_CONVERTER_VERSION
    }

    fn convert(
        &self,
        content: SourceContent<'_>,
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<ConversionOutcome, ConversionError> {
        let owned;
        let source = match content {
            SourceContent::Bytes(bytes) => bytes,
            SourceContent::Reader(reader) => {
                let limit = context.budgets().max_source_bytes;
                let mut bytes = Vec::new();
                reader
                    .take(limit.saturating_add(1))
                    .read_to_end(&mut bytes)
                    .map_err(ConversionError::Read)?;
                if bytes.len() as u64 > limit {
                    return Ok(ConversionOutcome::OverBudget {
                        budget: fm_semantic_conversion::BudgetKind::SourceBytes,
                        limit,
                    });
                }
                owned = bytes;
                &owned
            }
        };
        let outcome =
            self.deterministic
                .convert(SourceContent::Bytes(source), metadata, context)?;
        match outcome {
            ConversionOutcome::NoTextLayer { .. } => self.run_ocr(source, metadata, context),
            other => Ok(other),
        }
    }
}

fn terminate_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let process_group = rustix::process::Pid::from_child(child);
        let _terminate_result =
            rustix::process::kill_process_group(process_group, rustix::process::Signal::TERM);
        let grace_deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < grace_deadline {
            if child.try_wait().ok().flatten().is_some() {
                let _kill_result = rustix::process::kill_process_group(
                    process_group,
                    rustix::process::Signal::KILL,
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _kill_result =
            rustix::process::kill_process_group(process_group, rustix::process::Signal::KILL);
    }
    let _kill_result = child.kill();
    let _wait_result = child.wait();
}

fn configure_child_environment(
    command: &mut Command,
    temporary_directory: &Path,
    executable: &Path,
) {
    const SAFE_ENVIRONMENT: &[&str] = &[
        "LANG",
        "LC_ALL",
        "TESSDATA_PREFIX",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "COMSPEC",
    ];
    let retained = SAFE_ENVIRONMENT
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
        .collect::<Vec<_>>();
    command.env_clear();
    command.envs(retained);
    if let Some(path) = sanitized_executable_path(executable) {
        command.env("PATH", path);
    }
    command.env("TMPDIR", temporary_directory);
    command.env("TMP", temporary_directory);
    command.env("TEMP", temporary_directory);
}

fn configure_version_environment(command: &mut Command, executable: &Path) {
    const SAFE_ENVIRONMENT: &[&str] = &[
        "LANG",
        "LC_ALL",
        "SYSTEMROOT",
        "WINDIR",
        "PATHEXT",
        "COMSPEC",
    ];
    let retained = SAFE_ENVIRONMENT
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
        .collect::<Vec<_>>();
    command.env_clear();
    command.envs(retained);
    if let Some(path) = sanitized_executable_path(executable) {
        command.env("PATH", path);
    }
}

fn sanitized_executable_path(executable: &Path) -> Option<std::ffi::OsString> {
    let mut directories = sanitized_path_directories(std::env::var_os("PATH").as_deref());
    if let Some(parent) = executable.parent().filter(|parent| parent.is_absolute())
        && let Ok(parent) = parent.canonicalize()
        && parent.is_dir()
    {
        directories.retain(|directory| directory != &parent);
        directories.insert(0, parent);
    }
    std::env::join_paths(directories).ok()
}

#[cfg(unix)]
fn configure_child_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_child_process_group(_command: &mut Command) {}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|directory| executable_candidates(&directory, name))
        .find(|candidate| candidate.is_file())
}

#[cfg(not(windows))]
fn production_ocrmypdf_candidates() -> Vec<PathBuf> {
    let mut candidates = ocrmypdf_path_candidates(std::env::var_os("PATH").as_deref());
    candidates.extend(fixed_ocrmypdf_locations());
    candidates
}

#[cfg(windows)]
fn production_ocrmypdf_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
fn ocrmypdf_path_candidates(path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    sanitized_path_directories(path)
        .into_iter()
        .flat_map(|directory| executable_candidates(&directory, "ocrmypdf"))
        .collect()
}

fn sanitized_path_directories(path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut directories = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .filter(|directory| directory.is_absolute())
        .filter_map(|directory| directory.canonicalize().ok())
        .filter(|directory| directory.is_dir())
        .collect::<Vec<_>>();
    directories.dedup();
    directories
}

#[cfg(target_os = "macos")]
fn fixed_ocrmypdf_locations() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/ocrmypdf"),
        PathBuf::from("/usr/local/bin/ocrmypdf"),
    ]
}

#[cfg(target_os = "linux")]
fn fixed_ocrmypdf_locations() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/bin/ocrmypdf"),
        PathBuf::from("/usr/local/bin/ocrmypdf"),
        PathBuf::from("/snap/bin/ocrmypdf"),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn fixed_ocrmypdf_locations() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(unix)]
fn is_regular_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn is_regular_executable(path: &Path) -> bool {
    path.is_file()
        && path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
        })
}

#[cfg(not(any(unix, windows)))]
fn is_regular_executable(path: &Path) -> bool {
    path.is_file()
}

fn executable_candidates(directory: &Path, name: &str) -> Vec<PathBuf> {
    let direct = directory.join(name);
    #[cfg(windows)]
    {
        let extensions =
            std::env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let mut candidates = vec![direct];
        candidates.extend(
            extensions
                .to_string_lossy()
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| directory.join(format!("{name}{extension}"))),
        );
        candidates
    }
    #[cfg(not(windows))]
    {
        vec![direct]
    }
}

/// Builds Procyon's default converter with deterministic Docling PDF
/// extraction preferred and the baseline converter retained for every
/// unsupported or recoverably malformed input.
#[must_use]
pub fn converter_with_baseline_fallback() -> OptionalConverter {
    OptionalConverter::prefer_advanced(
        Arc::new(BaselineConverter::new()),
        Some(Arc::new(AdvancedConverterAdapter::new(Arc::new(
            DoclingPdfBackend::new(),
        )))),
    )
}

/// Builds the default converter and adds OCRmyPDF only when explicitly
/// configured by the trusted host.
#[must_use]
pub fn converter_with_optional_ocr(
    configuration: Option<OcrMyPdfConfiguration>,
) -> Arc<dyn DocumentConverter> {
    let deterministic: Arc<dyn DocumentConverter> = Arc::new(converter_with_baseline_fallback());
    match configuration {
        Some(configuration) => Arc::new(OcrMyPdfConverter::new(deterministic, configuration)),
        None => deterministic,
    }
}

#[cfg(feature = "ml")]
fn requested_ocr_assets(language: OcrLanguage) -> [(&'static str, String); 2] {
    let (recognizer, dictionary) = match language {
        OcrLanguage::English => (".models/ocr_rec_en.onnx", ".models/en_dict.txt"),
        OcrLanguage::Multilingual => (".models/ocr_rec.onnx", ".models/ppocr_keys_v1.txt"),
    };
    [
        (
            "ocr.rec",
            docling_core::env::nonempty("DOCLING_OCR_REC_ONNX")
                .unwrap_or_else(|| docling_core::assets::resolve(recognizer)),
        ),
        (
            "ocr.dict",
            docling_core::env::nonempty("DOCLING_OCR_DICT")
                .unwrap_or_else(|| docling_core::assets::resolve(dictionary)),
        ),
    ]
}

impl AdvancedConverterBackend for DoclingPdfBackend {
    fn version(&self) -> ComponentVersion {
        DOCLING_PDF_CONVERTER_VERSION
    }

    fn capabilities(&self) -> &[AdvancedCapability] {
        #[cfg(feature = "ml")]
        if self.pipeline.is_some() {
            return &[
                AdvancedCapability::Ocr,
                AdvancedCapability::ComplexLayout,
                AdvancedCapability::Tables,
            ];
        }
        &[AdvancedCapability::ComplexLayout]
    }

    fn convert_bytes(
        &self,
        bytes: &[u8],
        metadata: &DocumentMetadata,
        context: &ConversionContext,
    ) -> Result<AdvancedConversion, ConversionError> {
        if !is_pdf(bytes, metadata) {
            return Ok(AdvancedConversion {
                outcome: ConversionOutcome::Unsupported {
                    media_type: metadata.media_type().cloned(),
                    detail: "the Docling Adapter accepts PDF documents only".into(),
                },
                provenance_precision: ProvenancePrecision::Approximate,
            });
        }

        #[cfg(feature = "ml")]
        if let Some(pipeline) = &self.pipeline {
            return Ok(convert_ml(
                bytes,
                context,
                self.version(),
                pipeline,
                self.ocr_language.expect("ML mode has a language"),
            ));
        }

        Ok(convert_text_layer(bytes, context, self.version()))
    }
}

fn convert_text_layer(
    bytes: &[u8],
    context: &ConversionContext,
    version: ComponentVersion,
) -> AdvancedConversion {
    if let Some(outcome) = stopped(context) {
        return conversion(outcome);
    }

    let pdf = match lopdf::Document::load_mem(bytes) {
        Ok(pdf) => pdf,
        Err(error) => {
            return conversion(ConversionOutcome::Malformed {
                detail: format!("Docling could not parse the PDF container: {error}"),
            });
        }
    };
    if pdf.is_encrypted() {
        return conversion(ConversionOutcome::Encrypted {
            detail: "Docling does not open password-protected PDFs".into(),
        });
    }

    let total_pages = pdf.get_pages().len();
    if total_pages > 0 && context.budgets().max_items == 0 {
        return conversion(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Items,
            limit: 0,
        });
    }
    let selected_pages = total_pages.min(context.budgets().max_items as usize);
    let range = (selected_pages > 0).then_some((1, selected_pages));
    let document = match convert_text_layer_pages(bytes, "document.pdf", range) {
        Ok(document) => document,
        Err(error) => return conversion(pdf_error_outcome(error)),
    };
    if document.nodes.is_empty() {
        return conversion(no_text_layer());
    }

    let mut mapper = Mapper::new(version, context, total_pages, selected_pages, None);
    mapper.map_document(document);
    conversion(mapper.finish())
}

#[cfg(feature = "ml")]
fn convert_ml(
    bytes: &[u8],
    context: &ConversionContext,
    version: ComponentVersion,
    pipeline: &std::sync::Mutex<docling_pdf::Pipeline>,
    language: OcrLanguage,
) -> AdvancedConversion {
    if let Some(outcome) = stopped(context) {
        return conversion(outcome);
    }
    let pdf = match lopdf::Document::load_mem(bytes) {
        Ok(pdf) => pdf,
        Err(error) => {
            return conversion(ConversionOutcome::Malformed {
                detail: format!("Docling could not parse the PDF container: {error}"),
            });
        }
    };
    if pdf.is_encrypted() {
        return conversion(ConversionOutcome::Encrypted {
            detail: "Docling does not open password-protected PDFs".into(),
        });
    }
    let total_pages = pdf.get_pages().len();
    if total_pages > 0 && context.budgets().max_items == 0 {
        return conversion(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Items,
            limit: 0,
        });
    }
    let selected_pages = total_pages.min(context.budgets().max_items as usize);
    let mut pipeline = loop {
        match pipeline.try_lock() {
            Ok(pipeline) => break pipeline,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return conversion(ConversionOutcome::Malformed {
                    detail: "the Docling inference pipeline became unavailable".into(),
                });
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if let Some(outcome) = stopped(context) {
                    return conversion(outcome);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    };
    let mut combined = DoclingDocument::new("document.pdf");
    let mut omissions = Vec::new();
    let mut first_error = None;
    for page in 1..=selected_pages {
        if let Some(outcome) = stopped(context) {
            pipeline.set_pages(None);
            return conversion(outcome);
        }
        // One-page windows bound resident raster/tensor memory and create a
        // cancellation checkpoint around every inference unit.
        pipeline.set_pages(Some((page, page)));
        match pipeline.convert(bytes, None, "document.pdf") {
            Ok(mut document) => {
                combined.nodes.append(&mut document.nodes);
                combined.links.append(&mut document.links);
                if let Some(confidence) = document.confidence {
                    let target = combined.confidence.get_or_insert_with(Default::default);
                    target.pages.extend(confidence.pages);
                }
            }
            Err(error) => {
                let detail = format!("Docling could not convert PDF page {page}: {error}");
                if first_error.is_none() {
                    first_error = Some(error);
                }
                omissions.push(Omission::UnreadablePart { detail });
            }
        }
    }
    pipeline.set_pages(None);
    if combined.nodes.is_empty()
        && let Some(error) = first_error
    {
        return conversion(pdf_error_outcome(error));
    }

    let mut mapper = Mapper::new(
        version,
        context,
        total_pages,
        selected_pages,
        Some(language),
    );
    for omission in omissions {
        mapper.omit(omission);
    }
    mapper.map_document(combined);
    conversion(mapper.finish())
}

fn conversion(outcome: ConversionOutcome) -> AdvancedConversion {
    AdvancedConversion {
        outcome,
        // Procyon's current provenance represents the exact page and reading-order
        // block, but not Docling's region coordinates.
        provenance_precision: ProvenancePrecision::Approximate,
    }
}

fn no_text_layer() -> ConversionOutcome {
    ConversionOutcome::NoTextLayer {
        detail: OCR_REQUIRED_GUIDANCE.into(),
    }
}

fn pdf_error_outcome(error: PdfError) -> ConversionOutcome {
    let detail = error.to_string();
    if detail.to_ascii_lowercase().contains("password")
        || detail.to_ascii_lowercase().contains("encrypted")
    {
        ConversionOutcome::Encrypted { detail }
    } else {
        ConversionOutcome::Malformed { detail }
    }
}

fn stopped(context: &ConversionContext) -> Option<ConversionOutcome> {
    if context.is_cancelled() {
        Some(ConversionOutcome::Cancelled)
    } else if context.elapsed() > context.budgets().timeout {
        Some(ConversionOutcome::OverBudget {
            budget: fm_semantic_conversion::BudgetKind::Time,
            limit: context.budgets().timeout.as_millis() as u64,
        })
    } else {
        None
    }
}

struct Mapper<'a> {
    version: ComponentVersion,
    context: &'a ConversionContext,
    total_pages: usize,
    current_page: u32,
    block_index: u32,
    section_path: Vec<String>,
    units: Vec<StructuralUnit>,
    warnings: Vec<ConversionWarning>,
    omissions: Vec<Omission>,
    output_chars: u64,
    removed_characters: u32,
    saturated: bool,
    stopped: Option<ConversionOutcome>,
    ocr_language: Option<OcrLanguage>,
}

impl<'a> Mapper<'a> {
    fn new(
        version: ComponentVersion,
        context: &'a ConversionContext,
        total_pages: usize,
        selected_pages: usize,
        ocr_language: Option<OcrLanguage>,
    ) -> Self {
        let omissions = (selected_pages < total_pages)
            .then_some(Omission::ItemsDropped {
                item: "page",
                converted: selected_pages as u32,
                total: total_pages as u32,
            })
            .into_iter()
            .collect();
        Self {
            version,
            context,
            total_pages,
            current_page: 1,
            block_index: 0,
            section_path: Vec::new(),
            units: Vec::new(),
            warnings: Vec::new(),
            omissions,
            output_chars: 0,
            removed_characters: 0,
            saturated: false,
            stopped: None,
            ocr_language,
        }
    }

    fn map_document(&mut self, document: DoclingDocument) {
        if let Some(language) = self.ocr_language {
            let confidence = document
                .confidence
                .as_ref()
                .and_then(docling_core::confidence::ConfidenceReport::ocr_score)
                .map(Self::confidence_basis_points);
            self.warn(ConversionWarning::OcrAssessment {
                language: language.as_str().to_owned(),
                mean_confidence_basis_points: confidence,
            });
        }
        for node in document.nodes {
            if self.saturated || self.checkpoint().is_err() {
                break;
            }
            self.map_node(node, 0);
        }
    }

    fn confidence_basis_points(score: f64) -> u16 {
        (score.clamp(0.0, 1.0) * 10_000.0).round() as u16
    }

    fn map_node(&mut self, node: Node, depth: u32) {
        if self.saturated || self.checkpoint().is_err() {
            return;
        }
        if depth > self.context.budgets().max_nesting_depth {
            self.stopped = Some(ConversionOutcome::OverBudget {
                budget: fm_semantic_conversion::BudgetKind::NestingDepth,
                limit: u64::from(self.context.budgets().max_nesting_depth),
            });
            return;
        }
        match node {
            Node::PageInfo { page_no, .. } => {
                self.current_page = u32::try_from(page_no).unwrap_or(u32::MAX).max(1);
                self.block_index = 0;
            }
            Node::PageBreak => {}
            Node::Heading { level, text } => self.heading(level, text),
            Node::Paragraph { text } | Node::TextDump(text) => {
                if !self.is_page_number(&text) {
                    self.emit(UnitKind::Paragraph, text);
                }
            }
            Node::InlineGroup { md_text, .. } => self.emit(UnitKind::Paragraph, md_text),
            Node::CheckboxItem { checked, text } => {
                self.emit(
                    UnitKind::ListItem,
                    format!("[{}] {text}", if checked { "x" } else { " " }),
                );
            }
            Node::ListItem { text, .. } => self.emit(UnitKind::ListItem, text),
            Node::Code { text, pretty, .. } => {
                self.emit(UnitKind::CodeBlock, pretty.unwrap_or(text));
            }
            Node::Formula { latex, .. } => self.emit(UnitKind::CodeBlock, latex),
            Node::Table(table) => self.emit(UnitKind::Table, table_text(&table)),
            Node::Chart { caption, table, .. } => {
                let mut text = caption.unwrap_or_default();
                let rows = table_text(&table);
                if !text.is_empty() && !rows.is_empty() {
                    text.push('\n');
                }
                text.push_str(&rows);
                self.emit(UnitKind::Table, text);
            }
            Node::Picture { caption, .. } => {
                if let Some(caption) = caption {
                    self.emit(UnitKind::Paragraph, caption);
                } else {
                    self.warn(ConversionWarning::UnsupportedFeature {
                        detail: "an uncaptioned PDF image was not added to semantic text".into(),
                    });
                }
            }
            Node::FieldRegion { items } => {
                self.emit(UnitKind::Paragraph, field_region_text(&items));
            }
            Node::Group {
                layer, children, ..
            } => {
                if layer.is_none() {
                    for child in children {
                        self.map_node(child, depth.saturating_add(1));
                    }
                }
            }
            Node::Located { inner, .. } | Node::Commented { inner, .. } => {
                self.map_node(*inner, depth.saturating_add(1));
            }
            Node::Furniture { .. } | Node::PageFurniture { .. } | Node::CommentSection { .. } => {}
            Node::DoclangOnly(_) => self.warn(ConversionWarning::UnsupportedFeature {
                detail: "DocLang-only PDF content was not added to semantic text".into(),
            }),
        }
    }

    fn heading(&mut self, level: u8, text: String) {
        let level = usize::from(level.clamp(1, 6));
        self.section_path.truncate(level.saturating_sub(1));
        let parent_path = self.section_path.clone();
        if self.emit_with_path(UnitKind::Heading, text, parent_path)
            && let Some(heading) = self.units.last()
        {
            self.section_path.push(heading.text.clone());
        }
    }

    fn emit(&mut self, kind: UnitKind, text: String) {
        self.emit_with_path(kind, text, self.section_path.clone());
    }

    fn emit_with_path(&mut self, kind: UnitKind, text: String, section_path: Vec<String>) -> bool {
        if self.saturated || self.checkpoint().is_err() {
            return false;
        }
        if self.units.len() as u64 >= u64::from(self.context.budgets().max_units) {
            self.saturated = true;
            self.omit(Omission::UnitLimit {
                limit: self.context.budgets().max_units,
            });
            return false;
        }

        let sanitized = sanitize(text.trim(), 0);
        self.removed_characters = self.removed_characters.saturating_add(sanitized.removed);
        if sanitized.text.trim().is_empty() {
            return false;
        }
        let order = self.units.len() as u32;
        let mut text = sanitized.text;
        let mut truncated = false;
        let unit_limit = self.context.budgets().max_unit_chars as usize;
        if text.chars().count() > unit_limit {
            text = text.chars().take(unit_limit).collect();
            truncated = true;
            self.omit(Omission::UnitTruncated {
                unit_order: order,
                limit: self.context.budgets().max_unit_chars,
            });
        }

        let remaining = self
            .context
            .budgets()
            .max_output_chars
            .saturating_sub(self.output_chars);
        if text.chars().count() as u64 > remaining {
            self.saturated = true;
            self.omit(Omission::OutputCharLimit {
                limit: self.context.budgets().max_output_chars,
            });
            let keep = usize::try_from(remaining).unwrap_or(usize::MAX);
            if keep == 0 {
                return false;
            }
            text = text.chars().take(keep).collect();
            truncated = true;
        }
        self.output_chars = self
            .output_chars
            .saturating_add(text.chars().count() as u64);
        if let Some(excerpt) = instruction_like_excerpt(&text) {
            self.warn(ConversionWarning::InstructionLikeText {
                unit_order: order,
                excerpt,
            });
        }

        self.units.push(StructuralUnit {
            order,
            kind,
            format: FormatKind::Pdf,
            section_path,
            text,
            provenance: Provenance::PdfBlock {
                page_number: self.current_page,
                block_index: self.block_index,
            },
            boundary: TopLevelBoundary::Page(self.current_page),
            source_map: SourceMap::default(),
            truncated,
        });
        self.block_index = self.block_index.saturating_add(1);
        true
    }

    fn checkpoint(&mut self) -> Result<(), ()> {
        if let Some(outcome) = stopped(self.context) {
            self.stopped = Some(outcome);
            Err(())
        } else {
            Ok(())
        }
    }

    fn is_page_number(&self, text: &str) -> bool {
        text.trim().parse::<usize>().ok().is_some_and(|number| {
            number == self.current_page as usize && number <= self.total_pages
        })
    }

    fn warn(&mut self, warning: ConversionWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    fn omit(&mut self, omission: Omission) {
        if !self.omissions.contains(&omission) {
            self.omissions.push(omission);
        }
    }

    fn finish(mut self) -> ConversionOutcome {
        if let Some(outcome) = self.stopped {
            return outcome;
        }
        if self.removed_characters > 0 {
            let count = self.removed_characters;
            self.warn(ConversionWarning::RemovedInvisibleCharacters { count });
        }
        if self.units.is_empty() {
            return no_text_layer();
        }
        ConversionOutcome::Converted(ConvertedDocument::new(
            self.version,
            FormatKind::Pdf,
            self.units,
            self.warnings,
            self.omissions,
        ))
    }
}

fn table_text(table: &Table) -> String {
    let mut lines = Vec::new();
    if let Some(caption) = table
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lines.push(caption.to_owned());
    }
    lines.extend(table.rows.iter().map(|row| {
        row.iter()
            .map(|cell| cell.replace(['\n', '\t'], " ").trim().to_owned())
            .collect::<Vec<_>>()
            .join("\t")
    }));
    lines.join("\n")
}

fn field_region_text(items: &[FieldItem]) -> String {
    items
        .iter()
        .filter_map(|item| {
            let mut parts = Vec::new();
            if let Some(marker) = item
                .marker
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parts.push(marker.to_owned());
            }
            if let Some(key) = item.key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                parts.push(key.to_owned());
            }
            let mut text = parts.join(" ");
            if let Some(value) = item
                .value
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if !text.is_empty() {
                    text.push_str(": ");
                }
                text.push_str(value);
            }
            (!text.is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_pdf(bytes: &[u8], metadata: &DocumentMetadata) -> bool {
    bytes.starts_with(b"%PDF-")
        || metadata
            .media_type()
            .is_some_and(|media_type| media_type.as_str() == "application/pdf")
        || metadata.extension() == Some("pdf")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docling_core::Table;

    #[cfg(unix)]
    fn fake_version_executable(root: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let executable = root.join("ocrmypdf");
        fs::write(&executable, format!("#!/bin/sh\n{body}\n")).expect("write fake OCRmyPDF");
        let mut permissions = fs::metadata(&executable).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).expect("make executable");
        executable
    }

    #[test]
    fn maps_reading_order_sections_tables_and_page_provenance() {
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::PageInfo {
                    page_no: 1,
                    width: 612.0,
                    height: 792.0,
                },
                Node::Heading {
                    level: 1,
                    text: "Methods".into(),
                },
                Node::Paragraph {
                    text: "First column before second column.".into(),
                },
                Node::ListItem {
                    ordered: false,
                    number: 0,
                    first_in_list: true,
                    text: "Measured result".into(),
                    level: 0,
                    marker: None,
                    location: None,
                    dclx: None,
                    href: None,
                    layer: None,
                },
                Node::Table(Table {
                    rows: vec![
                        vec!["Metric".into(), "Value".into()],
                        vec!["Accuracy".into(), "98%".into()],
                    ],
                    ..Table::default()
                }),
                Node::PageFurniture {
                    footer: true,
                    location: [0, 0, 511, 10],
                    text: "Repeated footer".into(),
                },
                Node::PageInfo {
                    page_no: 2,
                    width: 612.0,
                    height: 792.0,
                },
                Node::Paragraph { text: "2".into() },
                Node::Paragraph {
                    text: "Continuation.".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: None,
        };
        let context = ConversionContext::new();
        let mut mapper = Mapper::new(DOCLING_PDF_CONVERTER_VERSION, &context, 2, 2, None);
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units().len(), 5);
        assert_eq!(converted.units()[1].section_path, ["Methods"]);
        assert_eq!(converted.units()[3].kind, UnitKind::Table);
        assert_eq!(converted.units()[3].text, "Metric\tValue\nAccuracy\t98%");
        assert_eq!(
            converted.units()[4].provenance,
            Provenance::PdfBlock {
                page_number: 2,
                block_index: 0
            }
        );
        assert!(
            converted
                .units()
                .iter()
                .all(|unit| !unit.text.contains("Repeated footer"))
        );
    }

    #[test]
    fn mapping_enforces_output_limits() {
        let context =
            ConversionContext::new().with_budgets(fm_semantic_conversion::ConversionBudgets {
                max_unit_chars: 4,
                max_output_chars: 6,
                ..fm_semantic_conversion::ConversionBudgets::default()
            });
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::Paragraph {
                    text: "abcdef".into(),
                },
                Node::Paragraph {
                    text: "ghijkl".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: None,
        };
        let mut mapper = Mapper::new(DOCLING_PDF_CONVERTER_VERSION, &context, 1, 1, None);
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units()[0].text, "abcd");
        assert_eq!(converted.units()[1].text, "gh");
        assert!(converted.is_partial());
    }

    #[test]
    fn heading_paths_are_sanitized_and_ocr_confidence_is_visible() {
        let mut pages = std::collections::BTreeMap::new();
        pages.insert(
            1,
            docling_core::confidence::PageConfidence {
                ocr_score: Some(0.8765),
                ..docling_core::confidence::PageConfidence::default()
            },
        );
        let document = DoclingDocument {
            name: "fixture".into(),
            nodes: vec![
                Node::Heading {
                    level: 1,
                    text: "Safe\u{202e} heading".into(),
                },
                Node::Paragraph {
                    text: "Body".into(),
                },
            ],
            strict_markdown: false,
            compact_tables: false,
            links: Vec::new(),
            confidence: Some(docling_core::confidence::ConfidenceReport::from_pages(
                pages,
            )),
        };
        let context = ConversionContext::new();
        let mut mapper = Mapper::new(
            DOCLING_PDF_CONVERTER_VERSION,
            &context,
            1,
            1,
            Some(OcrLanguage::English),
        );
        mapper.map_document(document);
        let outcome = mapper.finish();
        let converted = outcome.document().expect("converted");

        assert_eq!(converted.units()[1].section_path, ["Safe heading"]);
        assert!(
            converted
                .warnings()
                .contains(&ConversionWarning::OcrAssessment {
                    language: "en".into(),
                    mean_confidence_basis_points: Some(8765),
                })
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_discovery_reports_a_supported_version() {
        let directory = tempfile::tempdir().expect("discovery fixture");
        let executable = fake_version_executable(directory.path(), "printf 'ocrmypdf 16.10.4\\n'");

        assert_eq!(
            discover_ocrmypdf_from_candidates(
                vec![executable.clone()],
                Duration::from_secs(5),
                16 * 1024,
            ),
            OcrMyPdfAvailability::Available {
                executable: executable.canonicalize().expect("canonical executable"),
                version: OcrMyPdfVersion::new(16, 10, 4),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_discovery_distinguishes_missing_and_non_executable() {
        let directory = tempfile::tempdir().expect("discovery fixture");
        let missing = directory.path().join("missing");
        let unavailable = discover_ocrmypdf_from_candidates(
            vec![missing],
            Duration::from_secs(1),
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        );
        let OcrMyPdfAvailability::Unavailable { reason, guidance } = unavailable else {
            panic!("missing executable must be unavailable");
        };
        assert_eq!(reason, OcrMyPdfRejectionReason::Missing);
        assert!(guidance.contains("Procyon never downloads or installs it"));
        #[cfg(target_os = "macos")]
        assert!(guidance.contains("brew install ocrmypdf"));
        #[cfg(target_os = "linux")]
        assert!(guidance.contains("distribution package manager"));
        #[cfg(target_os = "windows")]
        assert!(guidance.contains("WSL"));

        let non_executable = directory.path().join("ocrmypdf");
        fs::write(&non_executable, "#!/bin/sh\nexit 0\n").expect("write non-executable");
        assert_eq!(
            discover_ocrmypdf_from_candidates(
                vec![non_executable.clone()],
                Duration::from_secs(1),
                OCRMYPDF_VERSION_OUTPUT_LIMIT,
            ),
            OcrMyPdfAvailability::Unavailable {
                reason: OcrMyPdfRejectionReason::NonExecutable {
                    path: non_executable.canonicalize().expect("canonical path"),
                },
                guidance: ocrmypdf_installation_guidance().into(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_discovery_rejects_malformed_and_unsupported_versions() {
        let malformed_directory = tempfile::tempdir().expect("malformed fixture");
        let malformed =
            fake_version_executable(malformed_directory.path(), "printf 'not-a-version\\n'");
        assert!(matches!(
            discover_ocrmypdf_from_candidates(
                vec![malformed],
                Duration::from_secs(5),
                OCRMYPDF_VERSION_OUTPUT_LIMIT,
            ),
            OcrMyPdfAvailability::Unavailable {
                reason: OcrMyPdfRejectionReason::MalformedVersion,
                ..
            }
        ));

        for reported in ["15.9.0", "16.0.0-rc.1", "18.0.0"] {
            let directory = tempfile::tempdir().expect("unsupported fixture");
            let executable =
                fake_version_executable(directory.path(), &format!("printf '{reported}\\n'"));
            assert_eq!(
                discover_ocrmypdf_from_candidates(
                    vec![executable],
                    Duration::from_secs(5),
                    OCRMYPDF_VERSION_OUTPUT_LIMIT,
                ),
                OcrMyPdfAvailability::Unavailable {
                    reason: OcrMyPdfRejectionReason::UnsupportedVersion {
                        version: OcrMyPdfVersion::parse(reported).expect("fixture version"),
                    },
                    guidance: ocrmypdf_installation_guidance().into(),
                }
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_discovery_types_an_unlaunchable_executable() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("unlaunchable fixture");
        let executable = directory.path().join("ocrmypdf");
        fs::write(&executable, b"\0not an executable image").expect("write invalid executable");
        let mut permissions = fs::metadata(&executable).expect("metadata").permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&executable, permissions).expect("make executable");

        let availability = discover_ocrmypdf_from_candidates(
            vec![executable],
            Duration::from_secs(3),
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        );
        assert!(
            matches!(
                availability,
                OcrMyPdfAvailability::Unavailable {
                    reason: OcrMyPdfRejectionReason::CouldNotExecute,
                    ..
                }
            ),
            "{availability:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_version_probe_times_out_and_terminates_descendants() {
        let directory = tempfile::tempdir().expect("timeout fixture");
        let marker = directory.path().join("descendant-survived");
        let executable = fake_version_executable(
            directory.path(),
            &format!(
                "(/bin/sleep 1; /usr/bin/touch '{}') &\n/bin/sleep 5",
                marker.display()
            ),
        );
        let started = Instant::now();

        assert!(matches!(
            discover_ocrmypdf_from_candidates(
                vec![executable],
                Duration::from_millis(75),
                OCRMYPDF_VERSION_OUTPUT_LIMIT,
            ),
            OcrMyPdfAvailability::Unavailable {
                reason: OcrMyPdfRejectionReason::TimedOut,
                ..
            }
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(1_100));
        assert!(!marker.exists(), "version-probe descendants must be killed");
    }

    #[cfg(unix)]
    #[test]
    fn successful_version_probe_does_not_leave_background_descendants() {
        let directory = tempfile::tempdir().expect("descendant fixture");
        let marker = directory.path().join("descendant-survived");
        let executable = fake_version_executable(
            directory.path(),
            &format!(
                "(trap '' TERM; /bin/sleep 1; /usr/bin/touch '{}') &\nprintf '16.10.4\\n'",
                marker.display()
            ),
        );
        let started = Instant::now();

        let availability = discover_ocrmypdf_from_candidates(
            vec![executable],
            Duration::from_secs(3),
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        );
        assert!(
            matches!(availability, OcrMyPdfAvailability::Available { .. }),
            "{availability:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(1_100));
        assert!(!marker.exists(), "version-probe descendants must be killed");
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_version_probe_rejects_oversized_output() {
        let directory = tempfile::tempdir().expect("oversized fixture");
        let executable =
            fake_version_executable(directory.path(), "printf '%9000s' x; printf '%9000s' y >&2");

        assert_eq!(
            discover_ocrmypdf_from_candidates(vec![executable], Duration::from_secs(5), 8 * 1024,),
            OcrMyPdfAvailability::Unavailable {
                reason: OcrMyPdfRejectionReason::OutputTooLarge { limit: 8 * 1024 },
                guidance: ocrmypdf_installation_guidance().into(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_version_probe_uses_only_the_fixed_argument_and_sanitized_environment() {
        let directory = tempfile::tempdir().expect("environment fixture");
        let executable = fake_version_executable(
            directory.path(),
            "[ \"$#\" -eq 1 ] && [ \"$1\" = \"--version\" ] || exit 41\n\
             [ -z \"${HOME+x}\" ] || exit 42\n\
             old_ifs=\"$IFS\"; IFS=:\n\
             for entry in $PATH; do case \"$entry\" in /*) ;; *) exit 43;; esac; done\n\
             IFS=\"$old_ifs\"\n\
             printf '17.1.0\\n'",
        );

        let availability = discover_ocrmypdf_from_candidates(
            vec![executable],
            Duration::from_secs(3),
            OCRMYPDF_VERSION_OUTPUT_LIMIT,
        );
        assert!(
            matches!(
                availability,
                OcrMyPdfAvailability::Available {
                    version: OcrMyPdfVersion {
                        major: 17,
                        minor: 1,
                        patch: 0,
                        pre_release: None,
                    },
                    ..
                }
            ),
            "{availability:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn production_ocr_path_search_ignores_relative_and_empty_entries() {
        let candidates = ocrmypdf_path_candidates(Some(std::ffi::OsStr::new(
            "relative::/usr/local/bin:/usr/bin",
        )));

        assert_eq!(
            candidates,
            [
                PathBuf::from("/usr/local/bin/ocrmypdf"),
                PathBuf::from("/usr/bin/ocrmypdf"),
            ]
        );
    }
}
