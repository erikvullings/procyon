//! Seekable media ranges of archive entries (task 0269).

use std::io::Write;

use fm_application::FileManagerService;
use fm_application::media_stream::{MediaOutcome, MediaRangeRequest};
use fm_domain::Location;
use fm_transport_dto::RuntimeKindDto;
use tokio::io::AsyncReadExt;
use zip::write::SimpleFileOptions;

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn archive_entry(archive: &std::path::Path, inner: &str) -> Location {
    let file = Location::from_native_path(archive).expect("location");
    Location::parse(&format!(
        "archive://{}!/{inner}",
        &file.uri["file://".len()..]
    ))
    .expect("archive entry location")
}

async fn bytes_of(service: &FileManagerService, location: Location, range: &str) -> (Vec<u8>, u64) {
    let outcome = service
        .open_media_range(MediaRangeRequest {
            location,
            range_header: Some(range.to_owned()),
            max_response_bytes: None,
        })
        .await
        .expect("media request opens");
    let MediaOutcome::Content(mut content) = outcome else {
        panic!("expected content");
    };
    let mut bytes = Vec::new();
    content.reader.read_to_end(&mut bytes).await.expect("read");
    (bytes, content.total_size)
}

#[tokio::test]
async fn media_in_stored_and_compressed_zip_entries_is_range_addressable() {
    let root = tempfile::tempdir().expect("temp dir");
    let content: Vec<u8> = (0..=250_u8).cycle().take(20_000).collect();
    let service = service(&root);
    for (name, method) in [
        ("stored.zip", zip::CompressionMethod::Stored),
        ("deflated.zip", zip::CompressionMethod::Deflated),
    ] {
        let path = root.path().join(name);
        let file = std::fs::File::create(&path).expect("create zip");
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(
                "music/track.flac",
                SimpleFileOptions::default().compression_method(method),
            )
            .expect("start entry");
        writer.write_all(&content).expect("write entry");
        writer.finish().expect("finish zip");

        let (middle, total) = bytes_of(
            &service,
            archive_entry(&path, "music/track.flac"),
            "bytes=12000-12009",
        )
        .await;
        assert_eq!(total, 20_000, "{name}");
        assert_eq!(middle, content[12_000..12_010], "{name}");
        let (tail, _) = bytes_of(
            &service,
            archive_entry(&path, "music/track.flac"),
            "bytes=-5",
        )
        .await;
        assert_eq!(tail, content[19_995..], "{name}");
    }
}
