//! Integration coverage for the read-only SQLite viewer REST surface (task 0251).

mod common;

use reqwest::StatusCode;
use serde_json::{Value, json};

async fn post(client: &reqwest::Client, base: &str, route: &str, body: Value) -> reqwest::Response {
    client
        .post(format!("{base}/api/v1/files/sqlite/{route}"))
        .json(&body)
        .send()
        .await
        .expect("request")
}

#[tokio::test]
async fn browses_searches_and_queries_a_local_database_without_modifying_it() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();
    let directory = tempfile::tempdir().expect("temp directory");
    let file = directory.path().join("data.sqlite");
    rusqlite::Connection::open(&file)
        .expect("create database")
        .execute_batch(
            "CREATE TABLE fruit (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO fruit (name) VALUES ('apple'), ('banana'), ('cherry');",
        )
        .expect("populate");
    let before = std::fs::read(&file).expect("read before");
    let location = json!({ "providerId": "local", "uri": format!("file://{}", file.display()) });

    let opened = post(
        &client,
        &server.base_url,
        "open",
        json!({ "location": location }),
    )
    .await;
    assert_eq!(opened.status(), StatusCode::OK);
    let opened: Value = opened.json().await.expect("open JSON");
    assert_eq!(opened["objects"][0]["name"], "fruit");
    let session = opened["sessionId"].clone();

    let page = post(
        &client,
        &server.base_url,
        "query",
        json!({
            "sessionId": session,
            "source": { "type": "browse", "object": "fruit", "search": "AN" },
            "limit": 10,
        }),
    )
    .await;
    assert_eq!(page.status(), StatusCode::OK);
    let page: Value = page.json().await.expect("query JSON");
    assert_eq!(page["totalRows"], 1);
    assert_eq!(page["rows"][0][1]["text"], "banana");

    let sql = post(
        &client,
        &server.base_url,
        "query",
        json!({
            "sessionId": session,
            "source": { "type": "sql", "sql": "SELECT count(*) AS n FROM fruit" },
            "limit": 10,
        }),
    )
    .await;
    let sql: Value = sql.json().await.expect("sql JSON");
    assert_eq!(sql["columns"][0], "n");
    assert_eq!(sql["rows"][0][0]["text"], "3");

    let write = post(
        &client,
        &server.base_url,
        "query",
        json!({
            "sessionId": session,
            "source": { "type": "sql", "sql": "DELETE FROM fruit" },
            "limit": 10,
        }),
    )
    .await;
    assert_eq!(write.status(), StatusCode::BAD_REQUEST);

    let closed = post(
        &client,
        &server.base_url,
        "close",
        json!({ "sessionId": session }),
    )
    .await;
    assert_eq!(closed.status(), StatusCode::NO_CONTENT);
    let after_close = post(
        &client,
        &server.base_url,
        "query",
        json!({ "sessionId": session, "source": { "type": "sql", "sql": "SELECT 1" }, "limit": 1 }),
    )
    .await;
    assert_eq!(after_close.status(), StatusCode::NOT_FOUND);
    assert_eq!(before, std::fs::read(&file).expect("read after"));
}

#[tokio::test]
async fn rejects_files_that_are_not_sqlite_databases() {
    let server = common::TestServer::spawn().await;
    let client = reqwest::Client::new();
    let directory = tempfile::tempdir().expect("temp directory");
    let file = directory.path().join("notes.db");
    std::fs::write(&file, "plain text, not a database").expect("fixture");
    let location = json!({ "providerId": "local", "uri": format!("file://{}", file.display()) });
    let response = post(
        &client,
        &server.base_url,
        "open",
        json!({ "location": location }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
