//! SSE transport contract tests for task 0032.

mod common;

use std::time::Duration;

use common::TestServer;
use fm_domain::Location;
use serde_json::{Value, json};
use uuid::Uuid;

async fn next_event(response: &mut reqwest::Response) -> String {
    next_event_with_timeout(response, Duration::from_secs(5)).await
}

async fn next_event_with_timeout(response: &mut reqwest::Response, timeout: Duration) -> String {
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::time::timeout(timeout, response.chunk())
            .await
            .expect("SSE event must arrive")
            .expect("stream read must succeed")
            .expect("stream must remain open");
        bytes.extend_from_slice(&chunk);
        if bytes.windows(2).any(|window| window == b"\n\n") {
            return String::from_utf8(bytes).expect("SSE must be UTF-8");
        }
    }
}

#[tokio::test]
async fn stream_emits_named_runtime_event_with_numeric_id_and_envelope() {
    let server = TestServer::spawn().await;
    let mut response = reqwest::Client::new()
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CONTENT_TYPE],
        "text/event-stream"
    );
    let event = next_event(&mut response).await;
    assert!(event.contains("event: runtime.ready\n"));
    assert!(
        event
            .lines()
            .any(|line| line.starts_with("id: ") && line[4..].parse::<u64>().is_ok())
    );
    let data = event
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    let envelope: Value = serde_json::from_str(data).expect("typed JSON envelope");
    assert_eq!(envelope["payload"]["type"], "runtime.ready");
    assert_eq!(
        envelope["eventId"].as_u64(),
        event
            .lines()
            .find_map(|line| line.strip_prefix("id: "))
            .unwrap()
            .parse::<u64>()
            .ok()
    );
}

#[tokio::test]
async fn idle_stream_emits_observable_named_keep_alive_event() {
    let server = TestServer::spawn().await;
    let mut response = reqwest::Client::new()
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    let _ = next_event(&mut response).await;

    let keep_alive = next_event_with_timeout(&mut response, Duration::from_secs(17)).await;

    assert!(keep_alive.contains("event: keep-alive\n"), "{keep_alive}");
}

#[tokio::test]
async fn reconnect_replays_retained_events_and_expired_id_resynchronises() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let mut first = client
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    let ready = next_event(&mut first).await;
    let ready_id: u64 = ready
        .lines()
        .find_map(|line| line.strip_prefix("id: "))
        .unwrap()
        .parse()
        .unwrap();
    drop(first);

    let mut replay = client
        .get(format!("{}/api/v1/events", server.base_url))
        .header("Last-Event-ID", ready_id.to_string())
        .send()
        .await
        .unwrap();
    assert!(
        next_event(&mut replay)
            .await
            .contains("event: runtime.ready\n")
    );

    for _ in 0..8 {
        let mut connection = client
            .get(format!("{}/api/v1/events", server.base_url))
            .send()
            .await
            .unwrap();
        let _ = next_event(&mut connection).await;
    }

    let mut gap = client
        .get(format!("{}/api/v1/events", server.base_url))
        .header("Last-Event-ID", "0")
        .send()
        .await
        .unwrap();
    assert!(
        next_event(&mut gap)
            .await
            .contains("event: resynchronise\n")
    );
}

#[tokio::test]
async fn browser_reconnect_query_replays_from_last_event_id() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let mut first = client
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    let ready = next_event(&mut first).await;
    let ready_id = ready
        .lines()
        .find_map(|line| line.strip_prefix("id: "))
        .unwrap();
    drop(first);

    let mut replay = client
        .get(format!(
            "{}/api/v1/events?lastEventId={ready_id}",
            server.base_url
        ))
        .send()
        .await
        .unwrap();

    assert!(
        next_event(&mut replay)
            .await
            .contains("event: runtime.ready\n")
    );
}

#[tokio::test]
async fn client_disconnect_releases_the_bus_subscription() {
    let server = TestServer::spawn().await;
    let mut response = reqwest::Client::new()
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    let _ = next_event(&mut response).await;
    assert_eq!(server.event_bus.subscriber_count(), 1);
    drop(response);
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.event_bus.subscriber_count() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("disconnect must release subscription");
}

#[tokio::test]
async fn development_session_end_closes_and_releases_the_subscription() {
    let server = TestServer::spawn().await;
    let mut response = reqwest::Client::new()
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    let _ = next_event(&mut response).await;
    assert_eq!(server.event_bus.subscriber_count(), 1);

    // Until task 0064 adds login/logout, the explicit development session's
    // lifetime is the server lifetime.
    server.session.end();
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.event_bus.subscriber_count() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("session end must release subscription");
}

#[tokio::test]
async fn directory_change_is_multiplexed_as_a_directory_delta() {
    let root = tempfile::tempdir().unwrap();
    let location = Location::from_native_path(root.path()).unwrap();
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let workspace: Value = client
        .post(format!("{}/api/v1/workspaces", server.base_url))
        .json(&json!({"name": "SSE test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let workspace_id = workspace["id"].as_str().unwrap();
    let pane_id = Uuid::new_v4();
    let mut stream = client
        .get(format!("{}/api/v1/events", server.base_url))
        .send()
        .await
        .unwrap();
    assert!(next_event(&mut stream).await.contains("runtime.ready"));
    let request = json!({
        "workspaceId": workspace_id, "paneId": pane_id, "requestId": Uuid::new_v4(),
        "location": {"providerId": location.provider_id.as_str(), "uri": location.uri},
    });
    client
        .post(format!("{}/api/v1/directories/list", server.base_url))
        .json(&request)
        .send()
        .await
        .unwrap();
    std::fs::write(root.path().join("new.txt"), b"new").unwrap();
    let event = next_event(&mut stream).await;
    assert!(event.contains("event: directory.delta\n"), "{event}");
}

#[tokio::test]
async fn disallowed_cross_origin_event_request_is_rejected() {
    let server = TestServer::spawn().await;
    let response = reqwest::Client::new()
        .get(format!("{}/api/v1/events", server.base_url))
        .header("Origin", "https://attacker.example")
        .send()
        .await
        .unwrap();
    assert_ne!(response.status(), reqwest::StatusCode::OK);
}
