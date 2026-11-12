//! HTTP coverage for LLM role bindings and endpoint consent (task 0322).

mod common;

use common::TestServer;
use serde_json::{Value, json};

fn profile_request() -> Value {
    json!({
        "name": "Cloud generation",
        "preset": "openAiCompatible",
        "baseUrl": "https://llm.example.test",
        "deployment": null,
        "apiVersion": null,
        "model": "model-a",
        "credential": null,
        "advanced": {
            "contextWindow": 8192,
            "maximumAnswerTokens": 1024,
            "temperature": 0.2,
            "timeoutSeconds": 30,
            "tlsPolicy": "requireHttps",
            "customHeaders": {}
        },
        "capabilities": ["chatCompletions"],
        "redactFilenames": true
    })
}

#[tokio::test]
async fn roles_bind_gate_cloud_embeddings_and_clear_with_their_profile() {
    let server = TestServer::spawn().await;
    let client = reqwest::Client::new();
    let base = &server.base_url;

    let created: Value = client
        .post(format!("{base}/api/v1/llm-profiles"))
        .json(&profile_request())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let profile_id = created["id"].as_str().unwrap().to_owned();

    let empty: Value = client
        .get(format!("{base}/api/v1/llm-roles"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(empty, json!({ "chat": null, "embeddings": null }));

    // A cloud endpoint may not serve embeddings until cloud use is allowed for that role.
    let blocked = client
        .put(format!("{base}/api/v1/llm-roles/embeddings"))
        .json(&json!({ "profileId": profile_id, "model": "embed", "allowCloud": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = blocked.json().await.unwrap();
    assert_eq!(body["details"]["reason"], "llmCloudEmbeddingNotAllowed");

    let allowed = client
        .put(format!("{base}/api/v1/llm-roles/embeddings"))
        .json(&json!({ "profileId": profile_id, "model": "embed", "allowCloud": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), reqwest::StatusCode::OK);
    let roles: Value = allowed.json().await.unwrap();
    assert_eq!(roles["embeddings"]["allowCloud"], true);
    assert_eq!(roles["chat"], Value::Null);

    // Consent is recorded for the saved host from the endpoint editor.
    let consented: Value = client
        .post(format!("{base}/api/v1/llm-profiles/{profile_id}/consent"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(consented["consentedHost"], "https://llm.example.test");

    let cleared: Value = client
        .delete(format!("{base}/api/v1/llm-roles/embeddings"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cleared["embeddings"], Value::Null);

    let unknown = client
        .put(format!("{base}/api/v1/llm-roles/imageGeneration"))
        .json(&json!({ "profileId": profile_id, "model": "m", "allowCloud": false }))
        .send()
        .await
        .unwrap();
    assert!(unknown.status().is_client_error());

    // Deleting the bound profile clears its role bindings.
    client
        .put(format!("{base}/api/v1/llm-roles/chat"))
        .json(&json!({ "profileId": profile_id, "model": "model-a", "allowCloud": false }))
        .send()
        .await
        .unwrap();
    let delete = client
        .delete(format!("{base}/api/v1/llm-profiles/{profile_id}"))
        .json(&json!({ "credentialDisposition": "delete" }))
        .send()
        .await
        .unwrap();
    assert!(delete.status().is_success());
    let after: Value = client
        .get(format!("{base}/api/v1/llm-roles"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after, json!({ "chat": null, "embeddings": null }));
}
