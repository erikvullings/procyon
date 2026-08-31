//! Public archive-provider behavior for task 0076.

use std::io::Write;

use fm_archive::{ArchiveFileSystemProvider, ArchiveLimits};
use fm_domain::{EntryKind, Location};
use fm_vfs::{FileSystemProvider, ListOptions, ProviderCapabilities, VfsError};
use rars::rar50::{Rar50Writer, StoredEntry, WriterOptions};
use tempfile::tempdir;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;
use zip::{ZipWriter, write::SimpleFileOptions};

fn zip_location(path: &std::path::Path) -> Location {
    let file = Location::from_native_path(path).expect("temporary path is absolute");
    Location::parse(&format!("archive://{}!", &file.uri["file://".len()..]))
        .expect("archive URI is valid")
}

#[tokio::test]
async fn zip_comic_aliases_are_detected_by_content_and_navigable() {
    let root = tempdir().expect("temporary root");
    for name in ["comic.cbz", "comic.cbr"] {
        let archive_path = root.path().join(name);
        let file = std::fs::File::create(&archive_path).expect("create comic fixture");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("001.jpg", SimpleFileOptions::default())
            .expect("start comic page");
        writer.write_all(b"image bytes").expect("write comic page");
        writer.finish().expect("finish comic fixture");

        let page = ArchiveFileSystemProvider::new()
            .list(
                &zip_location(&archive_path),
                ListOptions::default(),
                CancellationToken::new(),
            )
            .await
            .expect("list ZIP comic alias");
        assert_eq!(page.entries.len(), 1, "fixture {name}");
        assert_eq!(page.entries[0].name, "001.jpg", "fixture {name}");
    }
}

#[tokio::test]
async fn zip_comic_with_a_prepended_stub_is_detected_by_reader_fallback() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("comic.cbz");
    let mut bytes = b"comic launcher stub".to_vec();
    {
        let mut cursor = std::io::Cursor::new(&mut bytes);
        cursor.set_position(cursor.get_ref().len() as u64);
        let mut writer = ZipWriter::new(cursor);
        writer
            .start_file("001.jpg", SimpleFileOptions::default())
            .expect("start comic page");
        writer.write_all(b"image bytes").expect("write comic page");
        writer.finish().expect("finish comic fixture");
    }
    std::fs::write(&archive_path, bytes).expect("write comic fixture");

    let page = ArchiveFileSystemProvider::new()
        .list(
            &zip_location(&archive_path),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list ZIP comic with prepended bytes");
    assert_eq!(page.entries[0].name, "001.jpg");
}

fn tar_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut bytes);
        let content = b"tar report";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "docs/report.txt", content.as_slice())
            .expect("append tar entry");
        builder.finish().expect("finish tar fixture");
    }
    bytes
}

#[tokio::test]
async fn tar_family_is_detected_by_content_and_navigable_read_only() {
    let root = tempdir().expect("temporary root");
    let tar = tar_bytes();
    let mut fixtures: Vec<(&str, Vec<u8>)> = vec![("raw.bin", tar.clone())];

    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).expect("write gzip fixture");
    fixtures.push(("gzip.bin", gzip.finish().expect("finish gzip fixture")));

    let mut bzip = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    bzip.write_all(&tar).expect("write bzip2 fixture");
    fixtures.push(("bzip.bin", bzip.finish().expect("finish bzip2 fixture")));

    let mut xz = xz2::write::XzEncoder::new(Vec::new(), 6);
    xz.write_all(&tar).expect("write xz fixture");
    fixtures.push(("xz.bin", xz.finish().expect("finish xz fixture")));

    let provider = ArchiveFileSystemProvider::new();
    for (name, bytes) in fixtures {
        let archive_path = root.path().join(name);
        std::fs::write(&archive_path, bytes).expect("write tar-family fixture");
        let docs = zip_location(&archive_path)
            .join("docs")
            .expect("safe child");
        let page = provider
            .list(&docs, ListOptions::default(), CancellationToken::new())
            .await
            .expect("list tar-family directory");
        assert_eq!(page.entries[0].name, "report.txt", "fixture {name}");
        assert_eq!(page.entries[0].size, Some(10), "fixture {name}");
        let capabilities = provider
            .capabilities_for(&docs)
            .expect("detect tar capabilities");
        assert!(capabilities.contains(fm_vfs::ProviderCapabilities::READ));
        assert!(!capabilities.contains(fm_vfs::ProviderCapabilities::WRITE));

        let mut reader = provider
            .open_read(
                &fm_vfs::EntryRef {
                    id: fm_domain::EntryId::new(),
                    location: docs.join("report.txt").expect("entry location"),
                },
                CancellationToken::new(),
            )
            .await
            .expect("read tar-family entry");
        let mut content = Vec::new();
        reader
            .read_to_end(&mut content)
            .await
            .expect("read content");
        assert_eq!(content, b"tar report", "fixture {name}");
    }
}

#[tokio::test]
async fn standalone_gzip_is_exposed_as_a_single_read_only_file() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("chapter.txt.gz");
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(b"comic chapter")
        .expect("write gzip fixture");
    std::fs::write(&archive_path, gzip.finish().expect("finish gzip fixture"))
        .expect("write standalone gzip");
    let provider = ArchiveFileSystemProvider::new();
    let archive_root = zip_location(&archive_path);

    let page = provider
        .list(
            &archive_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list standalone gzip");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "chapter.txt");
    assert_eq!(page.entries[0].kind, EntryKind::File);

    let mut reader = provider
        .open_read(
            &fm_vfs::EntryRef {
                id: page.entries[0].id,
                location: archive_root.join("chapter.txt").expect("entry location"),
            },
            CancellationToken::new(),
        )
        .await
        .expect("read gzip member");
    let mut content = Vec::new();
    reader
        .read_to_end(&mut content)
        .await
        .expect("read content");
    assert_eq!(content, b"comic chapter");
    assert!(
        !provider
            .capabilities_for(&archive_root)
            .expect("gzip capabilities")
            .contains(ProviderCapabilities::WRITE)
    );
}

#[tokio::test]
async fn rar_comic_is_navigable_and_read_only() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("comic.cbr");
    let bytes = Rar50Writer::new(WriterOptions::default())
        .stored_entries(&[StoredEntry {
            name: b"pages/001.jpg",
            data: b"image bytes",
            mtime: Some(0),
            attributes: 0x20,
            host_os: 3,
        }])
        .finish()
        .expect("build RAR fixture");
    std::fs::write(&archive_path, bytes).expect("write RAR fixture");
    let provider = ArchiveFileSystemProvider::new();
    let location = zip_location(&archive_path);

    let capabilities = provider
        .capabilities_for(&location)
        .expect("detect RAR capabilities");
    assert!(capabilities.contains(ProviderCapabilities::LIST));
    assert!(capabilities.contains(ProviderCapabilities::READ));
    assert!(!capabilities.contains(ProviderCapabilities::WRITE));

    let pages = location.join("pages").expect("pages location");
    let page = provider
        .list(&pages, ListOptions::default(), CancellationToken::new())
        .await
        .expect("list RAR comic");
    assert_eq!(page.entries[0].name, "001.jpg");
    let mut reader = provider
        .open_read(
            &fm_vfs::EntryRef {
                id: page.entries[0].id,
                location: pages.join("001.jpg").expect("page location"),
            },
            CancellationToken::new(),
        )
        .await
        .expect("read RAR comic page");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).await.expect("read page");
    assert_eq!(content, b"image bytes");
}

#[tokio::test]
async fn reading_a_rar_comic_page_does_not_extract_later_pages() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("comic.cbr");
    let mut bytes = Rar50Writer::new(WriterOptions::default())
        .stored_entries(&[
            StoredEntry {
                name: b"001.jpg",
                data: b"first image bytes",
                mtime: Some(0),
                attributes: 0x20,
                host_os: 3,
            },
            StoredEntry {
                name: b"002.jpg",
                data: b"second image bytes",
                mtime: Some(0),
                attributes: 0x20,
                host_os: 3,
            },
        ])
        .finish()
        .expect("build RAR fixture");
    let trailing_page = bytes
        .windows(b"second image bytes".len())
        .position(|window| window == b"second image bytes")
        .expect("find trailing page payload");
    bytes[trailing_page] ^= 0xff;
    std::fs::write(&archive_path, bytes).expect("write RAR fixture");

    let provider = ArchiveFileSystemProvider::new();
    let mut reader = provider
        .open_read(
            &fm_vfs::EntryRef {
                id: fm_domain::EntryId::new(),
                location: zip_location(&archive_path)
                    .join("001.jpg")
                    .expect("page location"),
            },
            CancellationToken::new(),
        )
        .await
        .expect("read first page without decoding trailing page");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).await.expect("read page");
    assert_eq!(content, b"first image bytes");
}

#[tokio::test]
async fn committing_a_staged_file_transactionally_adds_it_to_a_zip() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    ZipWriter::new(std::fs::File::create(&archive_path).expect("create fixture"))
        .finish()
        .expect("finish fixture");
    let provider = ArchiveFileSystemProvider::new();
    let archive_root = zip_location(&archive_path);
    let temporary = archive_root
        .join(".fm-copy-test")
        .expect("temporary location");
    let destination = archive_root
        .join("report.txt")
        .expect("destination location");
    let mut stream = provider
        .open_write(
            &temporary,
            fm_vfs::WriteOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("open staged write");
    stream
        .write_all(b"quarterly report")
        .await
        .expect("write staged content");
    stream.shutdown().await.expect("flush staged content");
    drop(stream);

    provider
        .commit_copy(
            &fm_vfs::EntryRef {
                id: fm_domain::EntryId::new(),
                location: destination.clone(),
            },
            &temporary,
            &destination,
            fm_vfs::CopyCommitOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("publish archive rewrite");

    let page = provider
        .list(
            &archive_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list rewritten archive");
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["report.txt"]
    );
}

#[tokio::test]
async fn zip_entry_can_be_streamed_through_the_provider_interface() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("report.txt", SimpleFileOptions::default())
        .expect("start fixture entry");
    writer
        .write_all(b"quarterly report")
        .expect("write fixture");
    writer.finish().expect("finish fixture");

    let location = zip_location(&archive_path)
        .join("report.txt")
        .expect("entry location");
    let mut reader = ArchiveFileSystemProvider::new()
        .open_read(
            &fm_vfs::EntryRef {
                id: fm_domain::EntryId::new(),
                location,
            },
            CancellationToken::new(),
        )
        .await
        .expect("open archive entry");
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .expect("read archive entry");

    assert_eq!(bytes, b"quarterly report");
}

#[tokio::test]
async fn metadata_reports_compressed_size_and_compression_method_for_a_zip_entry() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file(
            "report.txt",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("start fixture entry");
    // Compressible content, so the deflated size is meaningfully smaller than the source.
    let content = b"quarterly report ".repeat(64);
    writer.write_all(&content).expect("write fixture");
    writer.finish().expect("finish fixture");

    let location = zip_location(&archive_path)
        .join("report.txt")
        .expect("entry location");
    let entry = fm_vfs::EntryRef {
        id: fm_domain::EntryId::new(),
        location,
    };
    let metadata = ArchiveFileSystemProvider::new()
        .metadata(&entry, CancellationToken::new())
        .await
        .expect("fetch archive entry metadata");

    let archive = metadata.archive.expect("archive metadata is populated");
    assert_eq!(archive.uncompressed_size, Some(content.len() as u64));
    let compressed_size = archive.compressed_size.expect("compressed size is known");
    assert!(compressed_size > 0 && compressed_size < content.len() as u64);
    assert_eq!(archive.compression_method.as_deref(), Some("Deflated"));
}

#[tokio::test]
async fn metadata_reports_no_archive_info_for_a_directory_entry() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .add_directory("photos/", SimpleFileOptions::default())
        .expect("start fixture directory");
    writer.finish().expect("finish fixture");

    let location = zip_location(&archive_path)
        .join("photos")
        .expect("entry location");
    let entry = fm_vfs::EntryRef {
        id: fm_domain::EntryId::new(),
        location,
    };
    let metadata = ArchiveFileSystemProvider::new()
        .metadata(&entry, CancellationToken::new())
        .await
        .expect("fetch archive entry metadata");

    assert!(metadata.archive.is_none());
}

#[tokio::test]
async fn encrypted_zip_distinguishes_missing_wrong_and_cached_correct_passwords() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("encrypted.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file(
            "secret.txt",
            SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "correct"),
        )
        .expect("start encrypted entry");
    writer.write_all(b"classified").expect("write fixture");
    writer.finish().expect("finish fixture");
    let provider = ArchiveFileSystemProvider::new();
    let root_location = zip_location(&archive_path);
    let entry_location = root_location.join("secret.txt").expect("entry location");
    let entry = fm_vfs::EntryRef {
        id: fm_domain::EntryId::new(),
        location: entry_location,
    };

    let missing = provider.open_read(&entry, CancellationToken::new()).await;
    assert!(matches!(missing, Err(VfsError::CredentialRequired)));

    provider
        .cache_password(&root_location, "wrong".to_owned())
        .expect("cache wrong password for this backend session");
    let wrong = provider.open_read(&entry, CancellationToken::new()).await;
    assert!(matches!(wrong, Err(VfsError::InvalidCredential)));

    provider
        .cache_password(&root_location, "correct".to_owned())
        .expect("replace cached password");
    let mut reader = provider
        .open_read(&entry, CancellationToken::new())
        .await
        .expect("retry with cached correct password");
    let mut content = Vec::new();
    reader.read_to_end(&mut content).await.expect("read secret");
    assert_eq!(content, b"classified");
}

#[tokio::test]
async fn zip_archive_is_navigable_as_directories_without_extracting_it() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("docs/report.txt", SimpleFileOptions::default())
        .expect("start fixture entry");
    writer
        .write_all(b"quarterly report")
        .expect("write fixture");
    writer.finish().expect("finish fixture");

    let provider = ArchiveFileSystemProvider::new();
    let archive_root = zip_location(&archive_path);
    let root_page = provider
        .list(
            &archive_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list archive root");
    assert_eq!(root_page.entries.len(), 1);
    assert_eq!(root_page.entries[0].name, "docs");
    assert_eq!(root_page.entries[0].kind, EntryKind::Directory);

    let docs_page = provider
        .list(
            &archive_root.join("docs").expect("safe child"),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list virtual directory");
    assert_eq!(docs_page.entries.len(), 1);
    assert_eq!(docs_page.entries[0].name, "report.txt");
    assert_eq!(docs_page.entries[0].kind, EntryKind::File);
    assert_eq!(docs_page.entries[0].size, Some(16));
    assert!(docs_page.entries[0].modified_at.is_some());
}

#[tokio::test]
async fn zip_inner_paths_with_spaces_are_navigable() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("sample.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("XIII Mysterie/cover.jpg", SimpleFileOptions::default())
        .expect("start fixture entry");
    writer.write_all(b"cover").expect("write fixture");
    writer.finish().expect("finish fixture");

    let provider = ArchiveFileSystemProvider::new();
    let archive_root = zip_location(&archive_path);
    let root_page = provider
        .list(
            &archive_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list archive root");
    assert_eq!(root_page.entries.len(), 1);
    assert_eq!(root_page.entries[0].name, "XIII Mysterie");
    assert_eq!(root_page.entries[0].kind, EntryKind::Directory);

    let nested_page = provider
        .list(
            &archive_root.join("XIII Mysterie").expect("safe child"),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list space-containing virtual directory");
    assert_eq!(nested_page.entries.len(), 1);
    assert_eq!(nested_page.entries[0].name, "cover.jpg");
    assert_eq!(nested_page.entries[0].kind, EntryKind::File);
}

#[tokio::test]
async fn seven_zip_archive_is_detected_by_content_and_navigable() {
    let root = tempdir().expect("temporary root");
    // Deliberately avoid a `.7z` suffix: provider format detection must inspect content.
    let archive_path = root.path().join("archive.bin");
    let mut writer = sevenz_rust2::ArchiveWriter::create(&archive_path).expect("create 7z fixture");
    writer
        .push_archive_entry(
            sevenz_rust2::ArchiveEntry::new_file("docs/über.txt"),
            Some(b"seven zip".as_slice()),
        )
        .expect("write 7z fixture entry");
    writer.finish().expect("finish 7z fixture");

    let provider = ArchiveFileSystemProvider::new();
    let docs = zip_location(&archive_path)
        .join("docs")
        .expect("safe child");
    let page = provider
        .list(&docs, ListOptions::default(), CancellationToken::new())
        .await
        .expect("list 7z directory");

    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "über.txt");
    assert_eq!(page.entries[0].size, Some(9));
    let capabilities = provider
        .capabilities_for(&docs)
        .expect("detect 7z capabilities");
    assert!(capabilities.contains(fm_vfs::ProviderCapabilities::READ));
    assert!(!capabilities.contains(fm_vfs::ProviderCapabilities::WRITE));
}

#[tokio::test]
async fn deleting_a_non_empty_zip_directory_rewrites_the_archive_tree() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("delete.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("keep.txt", SimpleFileOptions::default())
        .expect("start keep entry");
    writer.write_all(b"keep").expect("write keep entry");
    writer
        .start_file("docs/one.txt", SimpleFileOptions::default())
        .expect("start first child");
    writer.write_all(b"one").expect("write first child");
    writer
        .start_file("docs/nested/two.txt", SimpleFileOptions::default())
        .expect("start nested child");
    writer.write_all(b"two").expect("write nested child");
    writer.finish().expect("finish fixture");
    let provider = ArchiveFileSystemProvider::new();
    let archive_root = zip_location(&archive_path);
    let docs = archive_root.join("docs").expect("directory location");

    provider
        .remove(
            &fm_vfs::EntryRef {
                id: fm_domain::EntryId::new(),
                location: docs,
            },
            fm_vfs::RemoveOptions {
                recursive: true,
                use_trash: false,
            },
            CancellationToken::new(),
        )
        .await
        .expect("delete archive tree");

    let page = provider
        .list(
            &archive_root,
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect("list rewritten archive");
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["keep.txt"]
    );
}

#[tokio::test]
async fn opening_an_entry_over_the_uncompressed_limit_is_rejected_before_expansion() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("bomb.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("large.txt", SimpleFileOptions::default())
        .expect("start entry");
    writer.write_all(b"five!").expect("write entry");
    writer.finish().expect("finish fixture");
    let provider = ArchiveFileSystemProvider::with_limits(ArchiveLimits {
        max_uncompressed_entry_bytes: 4,
        max_expansion_ratio: 1_000,
    });
    let location = zip_location(&archive_path)
        .join("large.txt")
        .expect("entry location");

    let error = match provider
        .open_read(
            &fm_vfs::EntryRef {
                id: fm_domain::EntryId::new(),
                location,
            },
            CancellationToken::new(),
        )
        .await
    {
        Ok(_) => panic!("oversized expansion must be rejected"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        fm_vfs::VfsError::ArchiveResourceLimit {
            kind: "uncompressedEntryBytes"
        }
    ));
}

#[tokio::test]
async fn unsafe_archive_entry_paths_are_rejected_during_browsing() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("unsafe.zip");
    let file = std::fs::File::create(&archive_path).expect("create fixture");
    let mut writer = ZipWriter::new(file);
    writer
        .start_file("../escape.txt", SimpleFileOptions::default())
        .expect("start unsafe entry");
    writer.write_all(b"escape").expect("write entry");
    writer.finish().expect("finish fixture");

    let error = ArchiveFileSystemProvider::new()
        .list(
            &zip_location(&archive_path),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await
        .expect_err("unsafe path must reject archive");

    assert!(matches!(error, fm_vfs::VfsError::UnsafeArchiveEntry));
}

#[tokio::test]
async fn corrupt_zip_returns_a_typed_error_without_extracting_or_panicking() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("corrupt.bin");
    std::fs::write(&archive_path, b"PK\x03\x04truncated").expect("write corrupt fixture");

    let result = ArchiveFileSystemProvider::new()
        .list(
            &zip_location(&archive_path),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await;

    assert!(matches!(result, Err(VfsError::Io { .. })));
}

#[tokio::test]
async fn tar_link_entries_are_rejected_instead_of_being_followed() {
    let root = tempdir().expect("temporary root");
    let archive_path = root.path().join("links.tar");
    let file = std::fs::File::create(&archive_path).expect("create tar fixture");
    let mut builder = tar::Builder::new(file);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header
        .set_link_name("../../outside")
        .expect("set link target");
    header.set_cksum();
    builder
        .append_data(&mut header, "escape-link", std::io::empty())
        .expect("append symlink");
    builder.finish().expect("finish tar fixture");

    let result = ArchiveFileSystemProvider::new()
        .list(
            &zip_location(&archive_path),
            ListOptions::default(),
            CancellationToken::new(),
        )
        .await;

    assert!(matches!(result, Err(VfsError::UnsafeArchiveEntry)));
}
