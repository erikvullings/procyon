//! End-to-end music duplicate scan over a temporary tree (task 0265).

use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    LocationDto, MusicDuplicateConfidenceDto, MusicDuplicatePageDto, MusicDuplicateScanPhaseDto,
    RuntimeKindDto, StartMusicDuplicateScanRequestDto,
};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

/// Minimal PCM WAV; `level` changes the sample bytes so two files can differ.
fn wav(level: u8) -> Vec<u8> {
    let samples = 8000_u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + samples).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&8000_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&samples.to_le_bytes());
    bytes.extend(std::iter::repeat_n(level, samples as usize));
    bytes
}

async fn finished(service: &FileManagerService, scan_id: uuid::Uuid) -> MusicDuplicatePageDto {
    for _ in 0..500 {
        let page = service
            .get_music_duplicate_page(scan_id, 0, 50)
            .expect("page");
        if page.is_complete {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("scan did not finish");
}

#[tokio::test]
async fn finds_byte_identical_audio_and_ignores_other_files() {
    let root = tempfile::tempdir().expect("temp dir");
    let music = root.path().join("music");
    std::fs::create_dir_all(music.join("albums")).expect("dirs");
    std::fs::write(music.join("a.wav"), wav(100)).expect("a");
    std::fs::write(music.join("albums/copy of a.wav"), wav(100)).expect("copy");
    std::fs::write(music.join("b.wav"), wav(50)).expect("b");
    std::fs::write(music.join("notes.txt"), b"not audio").expect("notes");
    let service = service(&root);

    let started = service
        .start_music_duplicate_scan(StartMusicDuplicateScanRequestDto {
            roots: vec![LocationDto::from(
                Location::from_native_path(&music).expect("location"),
            )],
            show_hidden: false,
        })
        .expect("scan starts");
    let page = finished(&service, started.scan_id).await;

    assert_eq!(page.phase, MusicDuplicateScanPhaseDto::Done);
    assert_eq!(page.files_found, 3);
    assert!(!page.is_cancelled);
    assert_eq!(page.groups.len(), 1);
    let group = &page.groups[0];
    assert_eq!(group.confidence, MusicDuplicateConfidenceDto::Exact);
    assert_eq!(group.files.len(), 2);
    assert_eq!(group.reclaimable_bytes, group.files[0].size);
    assert!(group.files.iter().all(|file| file.lossless));
}

#[test]
fn unknown_scans_are_not_found() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    assert!(
        service
            .cancel_music_duplicate_scan(uuid::Uuid::new_v4())
            .is_err()
    );
    assert!(
        service
            .get_music_duplicate_page(uuid::Uuid::new_v4(), 0, 10)
            .is_err()
    );
}

#[tokio::test]
async fn an_empty_root_list_is_rejected() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    assert!(
        service
            .start_music_duplicate_scan(StartMusicDuplicateScanRequestDto {
                roots: vec![],
                show_hidden: false,
            })
            .is_err()
    );
}
