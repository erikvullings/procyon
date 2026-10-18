//! End-to-end broken-link scan over a temporary tree (task 0266).

use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    FindLinkReplacementsRequestDto, LinkDoctorPageDto, LinkProblemKindDto, LocationDto,
    RuntimeKindDto, StartLinkDoctorScanRequestDto,
};

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

async fn finished(service: &FileManagerService, scan_id: uuid::Uuid) -> LinkDoctorPageDto {
    for _ in 0..500 {
        let page = service.get_link_doctor_page(scan_id, 0, 100).expect("page");
        if page.is_complete {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("scan did not finish");
}

#[tokio::test]
async fn reports_each_kind_of_broken_link_and_ignores_healthy_ones() {
    let root = tempfile::tempdir().expect("temp dir");
    let tree = root.path().join("tree");
    std::fs::create_dir_all(tree.join("docs")).expect("dirs");
    std::fs::write(tree.join("docs/exists.md"), "# ok").expect("exists");
    std::fs::write(
        tree.join("readme.md"),
        "[ok](docs/exists.md) [gone](docs/missing.md) [web](https://x.test) [top](#a)\n![img](pics/none.png)",
    )
    .expect("readme");
    std::fs::write(
        tree.join("page.html"),
        "<a href=\"docs/exists.md\">ok</a><img src=\"nope.gif\">",
    )
    .expect("page");
    std::fs::write(
        tree.join("mix.m3u8"),
        "#EXTM3U\ndocs/exists.md\nlost song.mp3\nhttp://stream.test/x",
    )
    .expect("playlist");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("docs/exists.md", tree.join("good-link")).expect("good link");
        std::os::unix::fs::symlink("nowhere/at-all", tree.join("bad-link")).expect("bad link");
    }
    let service = service(&root);

    let started = service
        .start_link_doctor_scan(StartLinkDoctorScanRequestDto {
            roots: vec![location(&tree)],
            show_hidden: false,
        })
        .expect("scan starts");
    let page = finished(&service, started.scan_id).await;

    assert!(!page.is_cancelled);
    let mut found: Vec<(LinkProblemKindDto, String)> = page
        .problems
        .iter()
        .map(|problem| (problem.kind, problem.target.clone()))
        .collect();
    found.sort_by(|a, b| a.1.cmp(&b.1));
    let mut expected = vec![
        (
            LinkProblemKindDto::MissingDocumentLink,
            "docs/missing.md".to_owned(),
        ),
        (
            LinkProblemKindDto::MissingDocumentLink,
            "pics/none.png".to_owned(),
        ),
        (
            LinkProblemKindDto::MissingDocumentLink,
            "nope.gif".to_owned(),
        ),
        (
            LinkProblemKindDto::MissingPlaylistEntry,
            "lost song.mp3".to_owned(),
        ),
    ];
    #[cfg(unix)]
    expected.push((
        LinkProblemKindDto::BrokenSymlink,
        "nowhere/at-all".to_owned(),
    ));
    expected.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(found, expected);
    let missing_md = page
        .problems
        .iter()
        .find(|problem| problem.target == "docs/missing.md")
        .expect("missing.md problem");
    assert_eq!(missing_md.line, Some(1));
}

#[tokio::test]
async fn finds_replacement_candidates_ranked_by_size_and_name() {
    let root = tempfile::tempdir().expect("temp dir");
    let tree = root.path().join("tree");
    std::fs::create_dir_all(tree.join("a")).expect("a");
    std::fs::create_dir_all(tree.join("b")).expect("b");
    std::fs::write(tree.join("a/Song.mp3"), b"12345").expect("a");
    std::fs::write(tree.join("b/song.mp3"), b"123456789").expect("b");
    std::fs::write(tree.join("b/other.mp3"), b"1").expect("other");
    let service = service(&root);

    let response = service
        .find_link_replacements(FindLinkReplacementsRequestDto {
            roots: vec![location(&tree)],
            target_name: "song.mp3".to_owned(),
            expected_size: Some(9),
            show_hidden: false,
        })
        .await
        .expect("search");

    assert_eq!(response.candidates.len(), 2);
    assert!(response.candidates[0].same_size);
    assert!(response.candidates[0].exact_name);
    assert!(response.candidates[0].location.uri.ends_with("b/song.mp3"));
    assert!(!response.candidates[1].exact_name);
}

#[test]
fn rejects_requests_without_roots() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    assert!(
        service
            .cancel_link_doctor_scan(uuid::Uuid::new_v4())
            .is_err()
    );
}
