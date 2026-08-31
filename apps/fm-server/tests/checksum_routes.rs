//! End-to-end REST coverage for checksum calculation, checksum-file
//! save/verify and duplicate detection (spec §16 milestone 5, §18, §37,
//! task 0077).

mod common;

use common::TestServer;
use fm_domain::Location;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const SHA256_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const SHA256_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn location_json(path: &std::path::Path) -> Value {
    let location = Location::from_native_path(path).expect("temp path must be representable");
    json!({
        "providerId": location.provider_id.as_str(),
        "uri": location.uri,
    })
}

async fn start_checksums(
    client: &reqwest::Client,
    base_url: &str,
    paths: &[&std::path::Path],
    algorithms: &[&str],
) -> Value {
    let entries: Vec<Value> = paths.iter().map(|path| location_json(path)).collect();
    let response = client
        .post(format!("{base_url}/api/v1/checksums"))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "entries": entries,
            "algorithms": algorithms,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    response.json().await.expect("body must be JSON")
}

async fn poll_checksums_until_complete(
    client: &reqwest::Client,
    base_url: &str,
    job_id: &str,
) -> Value {
    for _ in 0..300 {
        let response = client
            .get(format!("{base_url}/api/v1/checksums/{job_id}?limit=500"))
            .send()
            .await
            .expect("request must succeed");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body: Value = response.json().await.expect("body must be JSON");
        if body["isComplete"] == json!(true) {
            return body;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("checksum job did not complete in time");
}

#[tokio::test]
async fn calculating_checksums_streams_results_through_the_paged_endpoint() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let alpha = root.path().join("alpha.txt");
    let beta = root.path().join("beta.txt");
    std::fs::write(&alpha, b"abc").unwrap();
    std::fs::write(&beta, b"").unwrap();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started = start_checksums(
        &client,
        &server.base_url,
        &[&alpha, &beta],
        &["sha256", "blake3"],
    )
    .await;
    let job_id = started["jobId"].as_str().unwrap();

    let page = poll_checksums_until_complete(&client, &server.base_url, job_id).await;
    assert_eq!(page["isCancelled"], json!(false));
    assert_eq!(page["totalEntries"], json!(2));
    let entries = page["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["relativePath"], json!("alpha.txt"));
    assert_eq!(entries[0]["checksums"]["sha256"], json!(SHA256_ABC));
    assert!(entries[0]["checksums"]["blake3"].is_string());
    assert_eq!(entries[1]["checksums"]["sha256"], json!(SHA256_EMPTY));
}

#[tokio::test]
async fn a_checksum_file_can_be_rendered_and_verified_against() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let alpha = root.path().join("alpha.txt");
    std::fs::write(&alpha, b"abc").unwrap();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let started = start_checksums(&client, &server.base_url, &[&alpha], &["sha256"]).await;
    let job_id = started["jobId"].as_str().unwrap();
    poll_checksums_until_complete(&client, &server.base_url, job_id).await;

    // Render the checksum file.
    let rendered: Value = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/checksum-file",
            server.base_url
        ))
        .json(&json!({"algorithm": "sha256"}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(rendered["suggestedName"], json!("checksums.sha256"));
    let content = rendered["content"].as_str().unwrap();
    assert!(
        content.contains(&format!("{SHA256_ABC}  alpha.txt")),
        "unexpected checksum file: {content}"
    );

    // Verifying against that same file must report a clean match.
    let report: Value = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/verify",
            server.base_url
        ))
        .json(&json!({"content": content}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(report["matched"], json!(1));
    assert_eq!(report["mismatched"], json!(0));
    assert_eq!(report["missing"], json!(0));
    assert_eq!(report["results"][0]["status"], json!("match"));

    // A tampered digest plus an entry that was never hashed must be reported
    // as a mismatch and a missing entry respectively.
    let tampered = format!(
        "{}  alpha.txt\n{SHA256_EMPTY}  never-hashed.txt\n",
        "0".repeat(64)
    );
    let report: Value = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/verify",
            server.base_url
        ))
        .json(&json!({"content": tampered}))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    assert_eq!(report["matched"], json!(0));
    assert_eq!(report["mismatched"], json!(1));
    assert_eq!(report["missing"], json!(1));
    assert_eq!(report["results"][0]["status"], json!("mismatch"));
    assert_eq!(report["results"][0]["actual"], json!(SHA256_ABC));
    assert_eq!(report["results"][1]["status"], json!("missing"));
}

#[tokio::test]
async fn a_checksum_file_is_written_to_disk_and_refuses_to_clobber() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let alpha = root.path().join("alpha.txt");
    std::fs::write(&alpha, b"abc").unwrap();
    let destination = root.path().join("checksums.sha256");

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let started = start_checksums(&client, &server.base_url, &[&alpha], &["sha256"]).await;
    let job_id = started["jobId"].as_str().unwrap();
    poll_checksums_until_complete(&client, &server.base_url, job_id).await;

    let saved = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/save",
            server.base_url
        ))
        .json(&json!({
            "destination": location_json(&destination),
            "algorithm": "sha256",
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(saved.status(), reqwest::StatusCode::CREATED);
    let body: Value = saved.json().await.expect("body must be JSON");
    assert!(body["bytesWritten"].as_u64().unwrap() > 0);

    // The file really exists on disk and is verifiable by `sha256sum --check`.
    let written = std::fs::read_to_string(&destination).expect("checksum file must exist");
    assert!(
        written.contains(&format!("{SHA256_ABC}  alpha.txt")),
        "unexpected checksum file: {written}"
    );
    assert_eq!(
        body["bytesWritten"].as_u64().unwrap() as usize,
        written.len()
    );

    // A second save must not silently destroy it.
    let clobber = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/save",
            server.base_url
        ))
        .json(&json!({
            "destination": location_json(&destination),
            "algorithm": "sha256",
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_ne!(
        clobber.status(),
        reqwest::StatusCode::CREATED,
        "overwriting must be opt-in"
    );

    // ... unless the caller explicitly opts in.
    let overwritten = client
        .post(format!(
            "{}/api/v1/checksums/{job_id}/save",
            server.base_url
        ))
        .json(&json!({
            "destination": location_json(&destination),
            "algorithm": "sha256",
            "overwrite": true,
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(overwritten.status(), reqwest::StatusCode::CREATED);
}

#[tokio::test]
async fn duplicate_detection_groups_identical_files_and_separates_hardlinks() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    std::fs::create_dir_all(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("one.txt"), b"identical payload").unwrap();
    std::fs::write(root.path().join("nested/two.txt"), b"identical payload").unwrap();
    // Same size, different content: must never be reported.
    std::fs::write(root.path().join("block-a.bin"), [b'A'; 128]).unwrap();
    std::fs::write(root.path().join("block-b.bin"), [b'B'; 128]).unwrap();
    // A hardlinked pair.
    let source = root.path().join("linked.dat");
    std::fs::write(&source, b"linked payload!!").unwrap();
    std::fs::hard_link(&source, root.path().join("nested/alias.dat")).unwrap();
    // A uniquely sized file that must never be hashed.
    std::fs::write(root.path().join("unique.txt"), b"xyz").unwrap();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();

    let started: Value = client
        .post(format!("{}/api/v1/duplicate-scans", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "roots": [location_json(root.path())],
        }))
        .send()
        .await
        .expect("request must succeed")
        .json()
        .await
        .expect("body must be JSON");
    let scan_id = started["scanId"].as_str().unwrap();

    let mut page = Value::Null;
    for _ in 0..300 {
        let body: Value = client
            .get(format!(
                "{}/api/v1/duplicate-scans/{scan_id}?limit=500",
                server.base_url
            ))
            .send()
            .await
            .expect("request must succeed")
            .json()
            .await
            .expect("body must be JSON");
        if body["isComplete"] == json!(true) {
            page = body;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(page != Value::Null, "duplicate scan did not complete");
    assert_eq!(page["isCancelled"], json!(false));

    let groups = page["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "unexpected groups: {groups:#?}");

    let identical = groups
        .iter()
        .find(|group| group["distinctLocations"].as_array().unwrap().len() == 2)
        .expect("the true-duplicate group must be reported");
    assert_eq!(identical["size"], json!(17));
    assert_eq!(identical["reclaimableBytes"], json!(17));
    assert!(identical["hardlinkClusters"].as_array().unwrap().is_empty());

    let linked = groups
        .iter()
        .find(|group| !group["hardlinkClusters"].as_array().unwrap().is_empty())
        .expect("the hardlinked pair must be reported distinctly");
    let clusters = linked["hardlinkClusters"].as_array().unwrap();
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0]["locations"].as_array().unwrap().len(), 2);
    // Hardlinks are one file: deleting a path reclaims nothing.
    assert_eq!(linked["reclaimableBytes"], json!(0));
    assert!(linked["distinctLocations"].as_array().unwrap().is_empty());

    // The staged funnel must never have fully hashed the uniquely sized file
    // nor the same-size-different-content pair.
    assert_eq!(page["stats"]["fullyHashed"], json!(3));
    assert_eq!(page["stats"]["sizeSurvivors"], json!(6));

    // No group may mention the same-size-different-content files.
    let serialized = serde_json::to_string(groups).unwrap();
    assert!(
        !serialized.contains("block-a.bin") && !serialized.contains("block-b.bin"),
        "same-size-different-content files must not be reported"
    );
}

#[tokio::test]
async fn a_checksum_job_can_be_cancelled_through_the_generic_operations_route() {
    let root = tempfile::tempdir().expect("must create a temp directory");
    let payload = vec![b'q'; 4 * 1024 * 1024];
    let paths: Vec<std::path::PathBuf> = (0..60)
        .map(|index| {
            let path = root.path().join(format!("f{index:02}.bin"));
            std::fs::write(&path, &payload).unwrap();
            path
        })
        .collect();
    let refs: Vec<&std::path::Path> = paths.iter().map(std::path::PathBuf::as_path).collect();

    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let started = start_checksums(&client, &server.base_url, &refs, &["sha256"]).await;
    let job_id = started["jobId"].as_str().unwrap();

    // The job id doubles as the operation id, so the generic cancel route
    // must reach it (task 0077 mirrors task 0075's id sharing).
    let cancelled = client
        .post(format!(
            "{}/api/v1/operations/{job_id}/cancel",
            server.base_url
        ))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(cancelled.status(), reqwest::StatusCode::NO_CONTENT);

    let page = poll_checksums_until_complete(&client, &server.base_url, job_id).await;
    assert_eq!(
        page["isCancelled"],
        json!(true),
        "a cancelled job must not report a clean completion"
    );
    assert!(
        page["total"].as_u64().unwrap() < 60,
        "cancellation must stop before every entry is hashed"
    );
}

#[tokio::test]
async fn an_unknown_job_or_scan_reports_not_found() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let unknown = Uuid::new_v4();

    for url in [
        format!("{}/api/v1/checksums/{unknown}", server.base_url),
        format!("{}/api/v1/duplicate-scans/{unknown}", server.base_url),
    ] {
        let response = client.get(&url).send().await.expect("request must succeed");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND,
            "unexpected status for {url}"
        );
    }
}

#[tokio::test]
async fn an_empty_selection_is_rejected() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/v1/checksums", server.base_url))
        .json(&json!({
            "workspaceId": Uuid::new_v4(),
            "entries": [],
            "algorithms": ["sha256"],
        }))
        .send()
        .await
        .expect("request must succeed");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
}
