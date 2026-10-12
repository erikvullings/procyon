//! Measures conversion of a generated PDF fixture without changing production limits.
//!
//! Generate the fixture with scripts/generate-semantic-pdf-fixtures.mjs in a
//! temporary directory. Pass only that synthetic text fixture to this probe.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use fm_semantic_conversion::{
    BaselineConverter, ConversionBudgets, ConversionContext, ConversionOutcome, DocumentConverter,
    DocumentMetadata, Provenance, SourceContent,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const SYNTHETIC_TEXT_DIGEST: &str =
    "b1e36f4b3e4664d00e09475372f9e90858aaef0adfab3762aaa0ea98cc4d2285";
const MAX_FIXTURE_BYTES: u64 = 200 * 1024 * 1024;
const MAX_PAGE_REPORT_BYTES: u64 = 8 * 1024 * 1024;
// Probe-only values, not production or release memory/deadline qualifications.
const PROBE_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
const PROBE_DEADLINE: Duration = Duration::from_secs(120);

struct KillOnDrop(Child);

#[derive(Serialize, Deserialize)]
struct PageText {
    number: u32,
    text: String,
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--child")) {
        return convert_child();
    }
    let path = env::args_os()
        .nth(1)
        .ok_or("pass a synthetic PDF fixture path")?;
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid fixture name")?;
    if name != "synthetic-354-page-text.pdf" {
        return Err("only the generated 354-page text fixture is accepted".into());
    }
    let source = fs::File::open(&path)?;
    let source_bytes = source.metadata()?.len();
    if !(175 * 1024 * 1024..=MAX_FIXTURE_BYTES).contains(&source_bytes) {
        return Err("synthetic fixture is outside the expected 175-200 MiB range".into());
    }
    let mut digest = Sha256::new();
    let mut reader = io::BufReader::new(source);
    let mut buffer = [0; 1024 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let digest = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != SYNTHETIC_TEXT_DIGEST {
        return Err("input bytes do not match the generated synthetic fixture".into());
    }
    let memory_cap = match env::args_os().nth(2).as_deref() {
        None => PROBE_MEMORY_BYTES,
        Some(value) if value == "--verify-kill" => 32 * 1024 * 1024,
        Some(_) => return Err("unsupported probe argument".into()),
    };
    let mut child = KillOnDrop(
        Command::new(env::current_exe()?)
            .arg("--child")
            .stdin(fs::File::open(&path)?)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let mut stdout = child.0.stdout.take().ok_or("missing child output")?;
    let output = thread::spawn(move || -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        stdout
            .by_ref()
            .take(MAX_PAGE_REPORT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let pid = Pid::from_u32(child.0.id());
    let mut system = System::new();
    let mut peak_rss = 0;
    let started = Instant::now();
    loop {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            false,
            ProcessRefreshKind::nothing().with_memory(),
        );
        if let Some(process) = system.process(pid) {
            peak_rss = peak_rss.max(process.memory());
            if peak_rss > memory_cap {
                return Err(format!("probe child exceeded sampled RSS cap: {peak_rss}").into());
            }
        } else if child.0.try_wait()?.is_none() && started.elapsed() > Duration::from_millis(100) {
            return Err("probe child memory could not be sampled".into());
        }
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                let mut error = String::new();
                child
                    .0
                    .stderr
                    .take()
                    .ok_or("missing child error")?
                    .read_to_string(&mut error)?;
                return Err(format!("probe child failed: {error}").into());
            }
            if peak_rss == 0 {
                return Err("probe child completed without a memory sample".into());
            }
            let output = output.join().map_err(|_| "child output reader failed")??;
            if output.len() as u64 > MAX_PAGE_REPORT_BYTES {
                return Err("child page report exceeded the bounded output limit".into());
            }
            let pages: Vec<PageText> = serde_json::from_slice(&output)?;
            if pages.len() != 354
                || pages.iter().enumerate().any(|(index, page)| {
                    page.number != index as u32 + 1
                        || !page
                            .text
                            .contains(&format!("Synthetic page {} of 354:", page.number))
                })
            {
                return Err("child page report is missing or misattributes text".into());
            }
            println!(
                "source_bytes={source_bytes} pages={} report_bytes={} sampled_peak_child_rss_bytes={peak_rss} parent_elapsed_ms={}",
                pages.len(),
                output.len(),
                started.elapsed().as_millis()
            );
            return Ok(());
        }
        if started.elapsed() >= PROBE_DEADLINE {
            return Err("probe child exceeded wall-clock deadline".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn convert_child() -> Result<(), Box<dyn std::error::Error>> {
    let mut source = Vec::new();
    io::stdin()
        .take(MAX_FIXTURE_BYTES + 1)
        .read_to_end(&mut source)?;
    if source.len() as u64 > MAX_FIXTURE_BYTES || hex_digest(&source) != SYNTHETIC_TEXT_DIGEST {
        return Err("child input does not match the bounded synthetic fixture".into());
    }
    let source_bytes = source.len();
    let budgets = ConversionBudgets {
        max_source_bytes: MAX_FIXTURE_BYTES,
        timeout: PROBE_DEADLINE,
        ..ConversionBudgets::default()
    };
    let started = Instant::now();
    let outcome = BaselineConverter::new().convert(
        SourceContent::Bytes(&source),
        &DocumentMetadata::unknown().with_media_type("application/pdf"),
        &ConversionContext::new().with_budgets(budgets),
    )?;
    let ConversionOutcome::Converted(document) = outcome else {
        return Err(format!("synthetic PDF conversion failed: {outcome:?}").into());
    };
    if document.is_partial() {
        return Err(format!(
            "synthetic PDF conversion was partial: {:?}",
            document.omissions()
        )
        .into());
    }
    let pages = document
        .units()
        .iter()
        .filter_map(|unit| match unit.provenance {
            Provenance::PdfBlock { page_number, .. } => Some(page_number),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    if pages.len() != 354 || pages.first() != Some(&1) || pages.last() != Some(&354) {
        return Err(format!("expected text on pages 1-354; found {pages:?}").into());
    }
    let report = pages
        .into_iter()
        .map(|number| PageText {
            number,
            text: document
                .units()
                .iter()
                .filter(|unit| matches!(unit.provenance, Provenance::PdfBlock { page_number, .. } if page_number == number))
                .map(|unit| unit.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .collect::<Vec<_>>();
    let output = serde_json::to_vec(&report)?;
    if output.len() as u64 > MAX_PAGE_REPORT_BYTES {
        return Err("child page report exceeded the bounded output limit".into());
    }
    io::stdout().write_all(&output)?;
    eprintln!(
        "converted synthetic source_bytes={source_bytes} text_units={} elapsed_ms={}",
        document.units().len(),
        started.elapsed().as_millis()
    );
    Ok(())
}
