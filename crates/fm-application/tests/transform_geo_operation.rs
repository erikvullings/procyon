//! `transformGeo` (task 0281): conversion jobs, conflicts, report and undo, in temp roots only.

use std::path::Path;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    ConvertGeoTextRequestDto, GeoTransformDto, LocationDto, OperationConflictPolicyDto,
    OperationDto, OperationKindDto, OperationStateDto, PreviewGeoConversionRequestDto,
    RuntimeKindDto, StartOperationRequestDto,
};
use uuid::Uuid;

const GPX: &str = r#"<gpx version="1.1"><wpt lat="52" lon="4"><name>A</name></wpt>
    <trk><name>Run</name><trkseg>
    <trkpt lat="52.0" lon="4.0"><ele>1</ele><time>2024-05-05T08:00:00Z</time></trkpt>
    <trkpt lat="52.1" lon="4.0"><ele>2</ele><time>2024-05-05T08:01:00Z</time></trkpt></trkseg></trk></gpx>"#;

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

async fn wait(service: &FileManagerService, id: Uuid) -> OperationDto {
    loop {
        let current = service.get_operation(id.into()).expect("operation");
        if matches!(
            current.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
        ) {
            return current;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn transform(
    service: &FileManagerService,
    files: &[&Path],
    format: &str,
    policy: OperationConflictPolicyDto,
) -> OperationDto {
    let operation = service
        .start_operation(
            StartOperationRequestDto {
                operation_type: OperationKindDto::TransformGeo,
                sources: files.iter().map(|path| location(path)).collect(),
                destination: None,
                destinations: vec![],
                conflict_policy: policy,
                name: None,
                archive_format: None,
                archive_compression_level: None,
                create_intermediate_directories: false,
                initial_content: None,
                skip_space_check: false,
                image_metadata_edit: None,
                geo_transform: Some(GeoTransformDto {
                    target_format: format.into(),
                    csv_wkt: false,
                }),
                symlink_policy: Default::default(),
                permanent_delete_confirmed: false,
                override_read_only: false,
                link: None,
            },
            None,
        )
        .expect("operation accepted");
    wait(service, operation.id).await
}

#[tokio::test]
async fn converts_next_to_the_source_reports_losses_and_can_be_undone() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("run.gpx");
    std::fs::write(&source, GPX).expect("fixture");
    let service = service(&root);

    let done = transform(&service, &[&source], "kml", OperationConflictPolicyDto::Ask).await;
    assert_eq!(done.state, OperationStateDto::CompletedWithWarnings);
    assert!(
        done.errors[0].message.contains("per-point timestamps"),
        "{:?}",
        done.errors
    );
    let output = std::fs::read_to_string(root.path().join("run.kml")).expect("output");
    assert!(output.contains("<name>Run</name>"));
    assert!(source.exists(), "the source is never touched");
    assert!(done.undo.available, "{:?}", done.undo.reason);

    let undo = service
        .undo_operation(done.id.into())
        .expect("undo accepted");
    assert_eq!(
        wait(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(!root.path().join("run.kml").exists());
    assert!(source.exists());
}

#[tokio::test]
async fn name_conflicts_follow_the_chosen_policy_and_never_overwrite_silently() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("run.gpx");
    std::fs::write(&source, GPX).expect("fixture");
    let existing = root.path().join("run.geojson");
    std::fs::write(&existing, "original").expect("existing output");
    let service = service(&root);

    let skipped = transform(
        &service,
        &[&source],
        "geojson",
        OperationConflictPolicyDto::Skip,
    )
    .await;
    assert_ne!(skipped.state, OperationStateDto::Failed);
    assert_eq!(
        std::fs::read_to_string(&existing).expect("kept"),
        "original"
    );

    let renamed = transform(
        &service,
        &[&source],
        "geojson",
        OperationConflictPolicyDto::RenameNew,
    )
    .await;
    assert_ne!(renamed.state, OperationStateDto::Failed);
    assert_eq!(
        std::fs::read_to_string(&existing).expect("kept"),
        "original"
    );
    let copies: Vec<_> = std::fs::read_dir(root.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("run (copy") && name.ends_with(".geojson"))
        .collect();
    assert_eq!(copies.len(), 1, "{copies:?}");

    let replaced = transform(
        &service,
        &[&source],
        "geojson",
        OperationConflictPolicyDto::Overwrite,
    )
    .await;
    assert_ne!(replaced.state, OperationStateDto::Failed);
    assert!(
        std::fs::read_to_string(&existing)
            .expect("replaced")
            .contains("FeatureCollection")
    );
    assert!(
        !replaced.undo.available,
        "a replaced file cannot be restored"
    );
}

#[tokio::test]
async fn a_file_that_cannot_be_read_fails_without_writing_anything() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("broken.gpx");
    std::fs::write(&source, "<gpx><wpt").expect("fixture");
    let service = service(&root);
    let done = transform(
        &service,
        &[&source],
        "geojson",
        OperationConflictPolicyDto::Ask,
    )
    .await;
    assert_eq!(done.state, OperationStateDto::Failed);
    let leftovers: Vec<_> = std::fs::read_dir(root.path())
        .expect("dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "broken.gpx" && name != "workspaces" && name != "settings")
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test]
async fn previews_report_losses_per_file_and_unreadable_files() {
    let root = tempfile::tempdir().expect("temporary directory");
    let source = root.path().join("run.gpx");
    std::fs::write(&source, GPX).expect("fixture");
    let broken = root.path().join("broken.kml");
    std::fs::write(&broken, "<kml><Placemark>").expect("fixture");
    let service = service(&root);
    let preview = service
        .preview_geo_conversion(PreviewGeoConversionRequestDto {
            locations: vec![location(&source), location(&broken)],
            target_format: "csv".into(),
            csv_wkt: false,
        })
        .await
        .expect("preview");
    assert_eq!(preview.reports[0].output_name.as_deref(), Some("run.csv"));
    assert!(
        preview.reports[0]
            .notes
            .iter()
            .any(|note| note.contains("left out"))
    );
    assert!(preview.reports[1].error.is_some());
    assert!(
        service
            .preview_geo_conversion(PreviewGeoConversionRequestDto {
                locations: vec![location(&source)],
                target_format: "shapefile".into(),
                csv_wkt: false,
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn converts_editor_text_but_not_to_kmz() {
    let root = tempfile::tempdir().expect("temporary directory");
    let service = service(&root);
    let text = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[4,52]},"properties":{"name":"A"}}"#;
    let converted = service
        .convert_geo_text(ConvertGeoTextRequestDto {
            text: text.into(),
            target_format: "gpx".into(),
            csv_wkt: false,
        })
        .await
        .expect("converted");
    assert_eq!(converted.extension, "gpx");
    assert!(converted.content.contains("<wpt lat=\"52\" lon=\"4\">"));
    assert!(
        service
            .convert_geo_text(ConvertGeoTextRequestDto {
                text: text.into(),
                target_format: "kmz".into(),
                csv_wkt: false,
            })
            .await
            .is_err()
    );
    assert!(
        service
            .convert_geo_text(ConvertGeoTextRequestDto {
                text: "{".into(),
                target_format: "kml".into(),
                csv_wkt: false,
            })
            .await
            .is_err()
    );
}
