//! Content search inside local ZIP archives.
//!
//! Scanning a ZIP means decompressing members, which can take far longer than reading plain files,
//! so the engine defers archives until every other file has been searched (see
//! `run_search_native`). Each member is sniffed from its first bytes; binary members (images, DICOM
//! data, ...) are skipped without decompressing the rest.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use fm_vfs::{ContentMatch, ContentQuery, looks_like_binary};
use tokio_util::sync::CancellationToken;
use zip::ZipArchive;

use crate::scanner::{extract_text, is_extractable, scan_text};

/// Largest member read in full for scanning.
const MAX_MEMBER_BYTES: u64 = 10 * 1024 * 1024;
/// Bytes read first to decide whether a member is worth reading in full.
const SNIFF_BYTES: u64 = 8192;

/// Content matches inside one archive member.
#[derive(Debug)]
pub(crate) struct MemberMatches {
    /// Path of the member inside the archive, `/`-separated.
    pub inner_path: String,
    /// Uncompressed size of the member.
    pub size: u64,
    pub matches: Vec<ContentMatch>,
}

/// Whether `path` is an archive this module can search.
pub(crate) fn is_searchable_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("zip") || extension.eq_ignore_ascii_case("cbz")
        })
}

/// Searches every member of the ZIP at `path` whose file name passes `accept_name`. Blocking: run
/// it on a blocking thread, `runtime` drives the async scanners.
pub(crate) fn scan_zip(
    path: &Path,
    query: &ContentQuery,
    accept_name: &dyn Fn(&str) -> bool,
    cancellation: &CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> Vec<MemberMatches> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(mut archive) = ZipArchive::new(BufReader::new(file)) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for index in 0..archive.len() {
        if cancellation.is_cancelled() {
            break;
        }
        let Ok(mut member) = archive.by_index(index) else {
            continue;
        };
        if member.is_dir() || member.encrypted() || member.size() > MAX_MEMBER_BYTES {
            continue;
        }
        let inner_path = member.name().to_owned();
        if inner_path.starts_with('/') || inner_path.split('/').any(|segment| segment == "..") {
            continue;
        }
        let file_name = inner_path.rsplit('/').next().unwrap_or("");
        if file_name.is_empty() || !accept_name(file_name) {
            continue;
        }
        let size = member.size();
        let mut bytes = Vec::new();
        if member
            .by_ref()
            .take(SNIFF_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.is_empty()
        {
            continue;
        }
        let extractable = is_extractable(&bytes);
        if !extractable && looks_like_binary(&bytes) {
            continue;
        }
        if member
            .take(MAX_MEMBER_BYTES)
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        let text = if extractable {
            match runtime.block_on(extract_text(bytes, cancellation)) {
                Ok(Some(text)) => text,
                _ => continue,
            }
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        if let Ok(result) = runtime.block_on(scan_text(&text, query, cancellation))
            && !result.matches.is_empty()
        {
            found.push(MemberMatches {
                inner_path,
                size,
                matches: result.matches,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn finds_text_and_documents_inside_a_zip_and_skips_binary_members() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bundle.zip");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default();
        writer.start_file("notes/plain.txt", options).unwrap();
        writer.write_all(b"first line\nStaal here\n").unwrap();
        writer.start_file("image.dcm", options).unwrap();
        writer.write_all(b"\0\0\0Staal\0\0").unwrap();
        writer.start_file("other.txt", options).unwrap();
        writer.write_all(b"nothing").unwrap();
        writer.finish().unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let query = ContentQuery::new("staal", false, false, false).unwrap();
        let found = scan_zip(
            &path,
            &query,
            &|_| true,
            &CancellationToken::new(),
            runtime.handle(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].inner_path, "notes/plain.txt");
        assert_eq!(found[0].matches[0].line_number, 2);

        let filtered = scan_zip(
            &path,
            &query,
            &|name| name.ends_with(".md"),
            &CancellationToken::new(),
            runtime.handle(),
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn recognises_zip_like_archives_case_insensitively() {
        assert!(is_searchable_archive(Path::new("a/B.ZIP")));
        assert!(is_searchable_archive(Path::new("comic.cbz")));
        assert!(!is_searchable_archive(Path::new("a.docx")));
    }
}
