use std::fs;
use std::sync::{Arc, Mutex};

use fm_settings::{Settings, SettingsStore};
use fm_transport_dto::{NotesSearchStrategyDto, SearchNotesRequestDto};

use super::store::tests::{location_of, registry};
use super::{NotesError, NotesService, notes_error};
use crate::error::ApplicationError;

fn service(settings_dir: &std::path::Path, suggested: Option<fm_domain::Location>) -> NotesService {
    NotesService::new(
        registry(),
        Arc::new(Mutex::new(Settings::default())),
        SettingsStore::new(settings_dir),
        suggested,
    )
}

#[tokio::test]
async fn setup_suggests_a_folder_until_one_is_chosen_and_the_choice_persists() {
    let settings_dir = tempfile::tempdir().unwrap();
    let notes_dir = tempfile::tempdir().unwrap();
    let suggested = location_of(notes_dir.path()).join("Procyon Notes").unwrap();
    let notes = service(settings_dir.path(), Some(suggested.clone()));

    let before = notes.setup().await;
    assert_eq!(before.root, None);
    assert!(!before.root_exists);
    assert_eq!(before.suggested_root, Some(suggested.into()));
    assert!(matches!(
        notes.store().await,
        Err(NotesError::NotConfigured)
    ));

    let chosen = location_of(notes_dir.path());
    let after = notes.set_root(chosen.clone()).await.unwrap();
    assert_eq!(after.root, Some(chosen.clone().into()));
    assert!(after.root_exists);
    let stored = SettingsStore::new(settings_dir.path())
        .load()
        .unwrap()
        .settings;
    assert_eq!(stored.notes_root, Some(chosen));
}

#[tokio::test]
async fn a_missing_folder_is_not_stored_and_a_server_suggests_nothing() {
    let settings_dir = tempfile::tempdir().unwrap();
    let notes_dir = tempfile::tempdir().unwrap();
    let notes = service(settings_dir.path(), None);
    let missing = location_of(notes_dir.path()).join("missing").unwrap();

    assert_eq!(
        notes.set_root(missing).await,
        Err(NotesError::RootUnavailable)
    );
    assert_eq!(notes.root(), None);
    assert_eq!(notes.setup().await.suggested_root, None);
}

#[tokio::test]
async fn the_service_search_reports_the_text_strategy_without_an_index() {
    let settings_dir = tempfile::tempdir().unwrap();
    let notes_dir = tempfile::tempdir().unwrap();
    fs::write(
        notes_dir.path().join("csr.md"),
        "# Create a CSR\nopenssl req -new\n",
    )
    .unwrap();
    let notes = service(settings_dir.path(), None);
    notes.set_root(location_of(notes_dir.path())).await.unwrap();
    let store = notes.store().await.unwrap();

    let result = notes
        .search(
            &store,
            None,
            &SearchNotesRequestDto {
                workspace_id: None,
                query: "  openssl csr ".into(),
                limit: Some(500),
            },
        )
        .await;
    assert_eq!(result.strategy, NotesSearchStrategyDto::Text);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].path, "csr.md");
    assert_eq!(result.hits[0].title, "Create a CSR");
}

#[test]
fn notes_failures_become_coded_rejections() {
    let ApplicationError::Rejected(rejection) =
        notes_error(NotesError::NotAvailable("../x.md".into()))
    else {
        panic!("expected a rejection");
    };
    assert_eq!(rejection.reason, "notesPathUnavailable");
    assert_eq!(
        rejection.params.get("path").map(String::as_str),
        Some("../x.md")
    );
}
