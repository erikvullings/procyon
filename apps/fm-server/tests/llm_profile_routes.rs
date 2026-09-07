//! HTTP parity and secret-redaction coverage for task 0184.

mod common;

use common::TestServer;
use serde_json::{Value, json};

fn profile_request(api_key: Option<&str>) -> Value {
    json!({
        "name": "Cloud generation",
        "preset": "openAiCompatible",
        "baseUrl": "https://llm.example.test",
        "deployment": null,
        "apiVersion": null,
        "model": "model-a",
        "credential": api_key.map(|api_key| json!({ "apiKey": api_key })),
        "advanced": {
            "contextWindow": 8192,
            "maximumAnswerTokens": 1024,
            "temperature": 0.2,
            "timeoutSeconds": 30,
            "tlsPolicy": "requireHttps",
            "customHeaders": {}
        },
        "capabilities": ["chatCompletions", "modelDiscovery"],
        "redactFilenames": true
    })
}

#[tokio::test]
async fn profile_crud_clone_export_and_test_never_echo_secret_material() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let secret = "task-0184-secret";

    let presets: Value = client
        .get(format!("{}/api/v1/llm-profiles/presets", server.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(presets.as_array().unwrap().len(), 7);

    let response = client
        .post(format!("{}/api/v1/llm-profiles", server.base_url))
        .json(&profile_request(Some(secret)))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CREATED);
    let raw = response.text().await.unwrap();
    assert!(!raw.contains(secret));
    assert!(!raw.to_ascii_lowercase().contains("credentialref"));
    let created: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(created["hasCredential"], true);
    assert_eq!(created["locality"], "cloud");
    let id = created["id"].as_str().unwrap();

    let tested: Value = client
        .post(format!("{}/api/v1/llm-profiles/{id}/test", server.base_url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tested["success"], false);
    assert_eq!(tested["category"], "policyDenied");
    assert!(!tested.to_string().contains(secret));

    let clone: Value = client
        .post(format!(
            "{}/api/v1/llm-profiles/{id}/clone",
            server.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(clone["hasCredential"], false);

    let exported: Value = client
        .get(format!(
            "{}/api/v1/llm-profiles/{id}/export",
            server.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let exported_text = exported.to_string().to_ascii_lowercase();
    assert!(!exported_text.contains("credential"));
    assert!(!exported_text.contains("consent"));

    let response = client
        .delete(format!("{}/api/v1/llm-profiles/{id}", server.base_url))
        .json(&json!({ "credentialDisposition": "delete" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
}
