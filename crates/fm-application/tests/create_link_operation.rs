//! Link creation operation tests (task 0168) confined to temporary roots.
#![expect(clippy::unwrap_used, reason = "temporary-root test setup")]

use std::{fs, io::Write, path::Path, time::Duration};

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_events::{BackendEventPayload, SessionId, SubscriptionEvent};
use fm_transport_dto::{
    ConflictResolutionDto, LinkKindDto, LinkOptionsRequestDto, LinkRequestDto, LinkTargetStyleDto,
    OperationConflictPolicyDto, OperationDto, OperationKindDto, OperationStateDto,
    ResolveOperationConflictRequestDto, RuntimeKindDto, StartOperationRequestDto,
};
use zip::{ZipWriter, write::SimpleFileOptions};

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &Path) -> Location {
    Location::from_native_path(path).unwrap()
}

fn options_request(target: Location, destination: Location) -> LinkOptionsRequestDto {
    LinkOptionsRequestDto {
        target: target.into(),
        destination: destination.into(),
    }
}

fn request(
    target: Location,
    destination: Location,
    name: &str,
    target_style: LinkTargetStyleDto,
    policy: OperationConflictPolicyDto,
) -> StartOperationRequestDto {
    StartOperationRequestDto {
        operation_type: OperationKindDto::CreateLink,
        sources: vec![target.into()],
        destination: Some(destination.into()),
        destinations: vec![],
        conflict_policy: policy,
        name: Some(name.to_owned()),
        archive_format: None,
        archive_compression_level: None,
        create_intermediate_directories: false,
        symlink_policy: Default::default(),
        permanent_delete_confirmed: false,
        override_read_only: false,
        link: Some(LinkRequestDto {
            kind: LinkKindDto::SymbolicLink,
            target_style,
        }),
    }
}

async fn settle(service: &FileManagerService, id: uuid::Uuid) -> OperationDto {
    for _ in 0..500 {
        let operation = service.get_operation(id.into()).unwrap();
        if matches!(
            operation.state,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::WaitingForConflictResolution
        ) {
            return operation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("link operation did not settle")
}

async fn run(service: &FileManagerService, request: StartOperationRequestDto) -> OperationDto {
    let started = service.start_operation(request, None).unwrap();
    settle(service, started.id).await
}

/// Runs a request expected to fail and returns its stable failure code.
async fn failure_code(service: &FileManagerService, request: StartOperationRequestDto) -> String {
    let mut events = service
        .event_bus()
        .subscribe_all_workspaces(SessionId::new("link-test"), None);
    let operation = run(service, request).await;
    assert_eq!(operation.state, OperationStateDto::Failed);
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("failure event")
            .unwrap();
        if let SubscriptionEvent::Event(envelope) = event
            && let BackendEventPayload::OperationFailed { code, .. } = envelope.payload
        {
            return code;
        }
    }
}

struct Fixture {
    root: tempfile::TempDir,
    targets: std::path::PathBuf,
    links: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let targets = root.path().join("doelen");
    let links = root.path().join("koppelingen");
    fs::create_dir(&targets).unwrap();
    fs::create_dir(&links).unwrap();
    fs::write(targets.join("doel ✓.txt"), b"target").unwrap();
    fs::create_dir(targets.join("map")).unwrap();
    fs::write(targets.join("map/child.txt"), b"child").unwrap();
    Fixture {
        root,
        targets,
        links,
    }
}

#[tokio::test]
async fn relative_unicode_file_link_is_stored_verbatim_and_undo_removes_only_the_link() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let target = fixture.targets.join("doel ✓.txt");

    let created = run(
        &service,
        request(
            location(&target),
            location(&fixture.links),
            "koppeling ✓",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Ask,
        ),
    )
    .await;

    assert_eq!(created.state, OperationStateDto::Completed);
    assert_eq!(created.operation_type, OperationKindDto::CreateLink);
    let link = fixture.links.join("koppeling ✓");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        Path::new("..").join("doelen").join("doel ✓.txt")
    );
    assert_eq!(fs::read(&link).unwrap(), b"target");
    assert!(created.undo.available);

    let undo = service.undo_operation(created.id.into()).unwrap();
    assert_eq!(
        settle(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"target");
}

#[tokio::test]
async fn absolute_directory_link_resolves_and_undo_keeps_the_directory_contents() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let target = fixture.targets.join("map");

    let created = run(
        &service,
        request(
            location(&target),
            location(&fixture.links),
            "map-link",
            LinkTargetStyleDto::Absolute,
            OperationConflictPolicyDto::Ask,
        ),
    )
    .await;

    assert_eq!(created.state, OperationStateDto::Completed);
    let link = fixture.links.join("map-link");
    assert_eq!(fs::read_link(&link).unwrap(), target);
    assert_eq!(fs::read(link.join("child.txt")).unwrap(), b"child");

    let undo = service.undo_operation(created.id.into()).unwrap();
    assert_eq!(
        settle(&service, undo.id).await.state,
        OperationStateDto::Completed
    );
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(fs::read(target.join("child.txt")).unwrap(), b"child");
}

#[tokio::test]
async fn an_occupied_name_asks_and_rename_new_keeps_the_existing_entry() {
    let fixture = fixture();
    let service = service(&fixture.root);
    fs::write(fixture.links.join("taken.txt"), b"existing").unwrap();

    let waiting = run(
        &service,
        request(
            location(&fixture.targets.join("doel ✓.txt")),
            location(&fixture.links),
            "taken.txt",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Ask,
        ),
    )
    .await;
    assert_eq!(
        waiting.state,
        OperationStateDto::WaitingForConflictResolution
    );

    service
        .resolve_operation_conflict(
            waiting.id.into(),
            ResolveOperationConflictRequestDto {
                resolution: ConflictResolutionDto::RenameNew,
                apply_to_all_similar: false,
            },
        )
        .unwrap();
    let mut state = OperationStateDto::WaitingForConflictResolution;
    for _ in 0..500 {
        state = service.get_operation(waiting.id.into()).unwrap().state;
        if state != OperationStateDto::WaitingForConflictResolution
            && state != OperationStateDto::Running
            && state != OperationStateDto::Queued
            && state != OperationStateDto::Planning
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(state, OperationStateDto::Completed);
    assert_eq!(
        fs::read(fixture.links.join("taken.txt")).unwrap(),
        b"existing"
    );
    assert_eq!(
        fs::read(fixture.links.join("taken (copy 1).txt")).unwrap(),
        b"target"
    );
}

#[tokio::test]
async fn skip_leaves_the_existing_entry_and_overwrite_replaces_only_a_file() {
    let fixture = fixture();
    let service = service(&fixture.root);
    fs::write(fixture.links.join("taken"), b"existing").unwrap();
    fs::create_dir(fixture.links.join("folder")).unwrap();
    let target = location(&fixture.targets.join("doel ✓.txt"));

    let skipped = run(
        &service,
        request(
            target.clone(),
            location(&fixture.links),
            "taken",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Skip,
        ),
    )
    .await;
    assert_eq!(skipped.state, OperationStateDto::Completed);
    assert_eq!(fs::read(fixture.links.join("taken")).unwrap(), b"existing");

    let replaced = run(
        &service,
        request(
            target.clone(),
            location(&fixture.links),
            "taken",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Overwrite,
        ),
    )
    .await;
    assert_eq!(replaced.state, OperationStateDto::Completed);
    assert!(
        fs::symlink_metadata(fixture.links.join("taken"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!replaced.undo.available);

    assert_eq!(
        failure_code(
            &service,
            request(
                target,
                location(&fixture.links),
                "folder",
                LinkTargetStyleDto::Relative,
                OperationConflictPolicyDto::Overwrite,
            ),
        )
        .await,
        "isADirectory"
    );
    assert!(fixture.links.join("folder").is_dir());
}

#[tokio::test]
async fn cycles_and_missing_targets_fail_with_typed_codes() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let directory = fixture.targets.join("map");

    assert_eq!(
        failure_code(
            &service,
            request(
                location(&directory),
                location(&directory),
                "loop",
                LinkTargetStyleDto::Relative,
                OperationConflictPolicyDto::Ask,
            ),
        )
        .await,
        "linkCycle"
    );
    assert!(fs::symlink_metadata(directory.join("loop")).is_err());

    assert_eq!(
        failure_code(
            &service,
            request(
                location(&fixture.targets.join("missing.txt")),
                location(&fixture.links),
                "dangling",
                LinkTargetStyleDto::Relative,
                OperationConflictPolicyDto::Ask,
            ),
        )
        .await,
        "notFound"
    );
    assert!(fs::symlink_metadata(fixture.links.join("dangling")).is_err());
}

/// A link name that resolves to the target itself (here through a symlinked parent; likewise a
/// case-only name change on a case-insensitive filesystem) must never replace the target.
#[cfg(unix)]
#[tokio::test]
async fn overwriting_an_entry_that_is_the_target_itself_fails_and_keeps_the_target() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let alias = fixture.links.join("alias");
    std::os::unix::fs::symlink(&fixture.targets, &alias).unwrap();
    let target = fixture.targets.join("doel ✓.txt");

    assert_eq!(
        failure_code(
            &service,
            request(
                location(&target),
                location(&alias),
                "doel ✓.txt",
                LinkTargetStyleDto::Absolute,
                OperationConflictPolicyDto::Overwrite,
            ),
        )
        .await,
        "linkCycle"
    );
    let metadata = fs::symlink_metadata(&target).unwrap();
    assert!(metadata.is_file());
    assert_eq!(fs::read(&target).unwrap(), b"target");
}

#[cfg(unix)]
#[tokio::test]
async fn a_read_only_destination_fails_with_permission_denied() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture();
    let service = service(&fixture.root);
    fs::set_permissions(&fixture.links, fs::Permissions::from_mode(0o555)).unwrap();

    let code = failure_code(
        &service,
        request(
            location(&fixture.targets.join("doel ✓.txt")),
            location(&fixture.links),
            "denied",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Ask,
        ),
    )
    .await;

    fs::set_permissions(&fixture.links, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(code, "permissionDenied");
    assert!(fs::symlink_metadata(fixture.links.join("denied")).is_err());
}

#[tokio::test]
async fn link_options_offer_symbolic_links_locally() {
    let fixture = fixture();
    let service = service(&fixture.root);

    let options = service
        .link_options(options_request(
            location(&fixture.targets.join("doel ✓.txt")),
            location(&fixture.links),
        ))
        .await
        .unwrap();

    let symlink = options
        .kinds
        .iter()
        .find(|option| option.kind == LinkKindDto::SymbolicLink)
        .expect("symbolic links are supported locally");
    assert!(symlink.supports_relative);
    assert_eq!(symlink.suggested_name, "doel ✓.txt");
    if !cfg!(windows) {
        assert_eq!(options.kinds.len(), 1, "{options:?}");
    }
}

#[tokio::test]
async fn providers_without_link_semantics_are_gated() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let archive = fixture.root.path().join("bundle.zip");
    {
        let mut writer = ZipWriter::new(fs::File::create(&archive).unwrap());
        writer
            .start_file("inside.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"inside").unwrap();
        writer.finish().unwrap();
    }
    let archive_file = location(&archive);
    let archive_root = Location::parse(&format!(
        "archive://{}!",
        &archive_file.uri["file://".len()..]
    ))
    .unwrap();
    let inside = archive_root.join("inside.txt").unwrap();

    let options = service
        .link_options(options_request(inside.clone(), archive_root.clone()))
        .await
        .unwrap();
    assert!(options.kinds.is_empty(), "{options:?}");

    let rejected = service.start_operation(
        request(
            inside.clone(),
            archive_root,
            "link",
            LinkTargetStyleDto::Relative,
            OperationConflictPolicyDto::Ask,
        ),
        None,
    );
    assert!(rejected.is_err(), "{rejected:?}");

    let cross_provider = service
        .link_options(options_request(inside, location(&fixture.links)))
        .await
        .unwrap();
    assert!(cross_provider.kinds.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn resolve_link_target_follows_symlinks_and_returns_other_entries_unchanged() {
    let fixture = fixture();
    let service = service(&fixture.root);
    let target = fixture.targets.join("doel ✓.txt");
    let link = fixture.links.join("data.json");
    std::os::unix::fs::symlink("../doelen/doel ✓.txt", &link).unwrap();

    let resolved = service
        .resolve_link_target(fm_transport_dto::ResolveLinkTargetRequestDto {
            location: location(&link).into(),
        })
        .await
        .unwrap();
    assert_eq!(resolved.kind, fm_transport_dto::EntryKindDto::File);
    assert_eq!(resolved.size, Some(6));
    assert_eq!(
        Location::from(resolved.location).to_native_path().unwrap(),
        fs::canonicalize(&target).unwrap()
    );

    let plain = service
        .resolve_link_target(fm_transport_dto::ResolveLinkTargetRequestDto {
            location: location(&target).into(),
        })
        .await
        .unwrap();
    assert_eq!(plain.kind, fm_transport_dto::EntryKindDto::File);
    assert_eq!(plain.name, "doel ✓.txt");
}
