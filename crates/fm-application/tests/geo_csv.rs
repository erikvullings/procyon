//! CSV files read as geographic data through the service.

use std::fs;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{CsvGeoOptionsDto, ReadGeoFileRequestDto, RuntimeKindDto};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

#[tokio::test]
async fn a_csv_with_rd_coordinates_is_converted_and_reports_how_it_was_read() {
    let root = tempfile::tempdir().expect("temporary root");
    let file = root.path().join("places.csv");
    fs::write(
        &file,
        "name;X;Y\nAmersfoort;155000;463000\nUtrecht;136900;456100\n",
    )
    .unwrap();
    let service = service(&root);
    let location = Location::from_native_path(&file).unwrap();

    let document = service
        .read_geo_file(ReadGeoFileRequestDto {
            location: location.clone().into(),
            csv: None,
        })
        .await
        .expect("read");

    assert_eq!(document.format, "csv");
    assert!(document.error.is_none());
    assert_eq!(document.displayed_features, 2);
    let info = document.csv.expect("csv info");
    assert_eq!(info.crs.as_deref(), Some("EPSG:28992"));
    assert!(info.crs_guessed);
    assert_eq!(info.headers, ["name", "X", "Y"]);
    let longitude = document.collection["features"][0]["geometry"]["coordinates"][0]
        .as_f64()
        .unwrap();
    assert!((longitude - 5.3872).abs() < 0.001);

    // The same file with another system chosen explicitly.
    let utm = service
        .read_geo_file(ReadGeoFileRequestDto {
            location: location.into(),
            csv: Some(CsvGeoOptionsDto {
                crs: Some("EPSG:32631".into()),
                ..CsvGeoOptionsDto::default()
            }),
        })
        .await
        .expect("read");
    assert_eq!(utm.csv.expect("info").crs.as_deref(), Some("EPSG:32631"));
}

#[tokio::test]
async fn a_csv_without_coordinates_still_lists_its_columns() {
    let root = tempfile::tempdir().expect("temporary root");
    let file = root.path().join("people.csv");
    fs::write(&file, "name,age\nAnn,3\n").unwrap();

    let document = service(&root)
        .read_geo_file(ReadGeoFileRequestDto {
            location: Location::from_native_path(&file).unwrap().into(),
            csv: None,
        })
        .await
        .expect("read");

    assert!(document.error.is_some());
    assert_eq!(document.csv.expect("columns").headers, ["name", "age"]);
}
