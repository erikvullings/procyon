//! Metadata token resolution over real files (task 0267).

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{LocationDto, ResolveMetadataTokensRequestDto, RuntimeKindDto};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &std::path::Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::new_rgb8(width, height)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

#[tokio::test]
async fn resolves_available_tokens_and_omits_missing_ones_in_request_order() {
    let root = tempfile::tempdir().expect("temp dir");
    let photo = root.path().join("holiday.png");
    let text = root.path().join("notes.txt");
    std::fs::write(&photo, png(40, 30)).expect("photo");
    std::fs::write(&text, b"hello").expect("text");
    let service = service(&root);

    let response = service
        .resolve_metadata_tokens(ResolveMetadataTokensRequestDto {
            locations: vec![location(&photo), location(&text)],
            tokens: vec![
                "exif:dimensions".to_owned(),
                "exif:date-taken:yyyy".to_owned(),
                "artist".to_owned(),
                "bogus".to_owned(),
            ],
        })
        .await
        .expect("resolve");

    assert_eq!(response.unknown_tokens, vec!["bogus".to_owned()]);
    assert_eq!(response.entries.len(), 2);
    assert_eq!(
        response.entries[0]
            .values
            .get("exif:dimensions")
            .map(String::as_str),
        Some("40x30")
    );
    // No EXIF date in a bare PNG and no tags in an image: those tokens are simply absent.
    assert!(
        !response.entries[0]
            .values
            .contains_key("exif:date-taken:yyyy")
    );
    assert!(!response.entries[0].values.contains_key("artist"));
    assert!(response.entries[1].values.is_empty());
}

#[tokio::test]
async fn refuses_oversized_requests() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let file = location(&root.path().join("a.mp3"));

    let too_many_files = service
        .resolve_metadata_tokens(ResolveMetadataTokensRequestDto {
            locations: vec![file.clone(); 501],
            tokens: vec!["artist".to_owned()],
        })
        .await;
    assert!(too_many_files.is_err());

    let too_many_tokens = service
        .resolve_metadata_tokens(ResolveMetadataTokensRequestDto {
            locations: vec![file],
            tokens: vec!["artist".to_owned(); 33],
        })
        .await;
    assert!(too_many_tokens.is_err());
}
