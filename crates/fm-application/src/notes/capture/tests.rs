use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::TimeZone as _;
use fm_transport_dto::{ChatFailureDto, TerminalBlockDto};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole};
use crate::notes::format::split_note;

struct Fake {
    answers: Mutex<VecDeque<Result<String, LlmProfileError>>>,
    seen: Mutex<Vec<(LlmRole, LlmChatGeneration)>>,
}

#[async_trait]
impl ChatGenerator for Fake {
    async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
        Ok(false)
    }

    async fn generate_for_role(
        &self,
        role: LlmRole,
        generation: LlmChatGeneration,
        _cancellation: &CancellationToken,
        _on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, LlmProfileError> {
        self.seen.lock().unwrap().push((role, generation));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok("unscripted".into()))
    }
}

fn fake(answer: Result<&str, LlmProfileError>) -> Fake {
    Fake {
        answers: Mutex::new(VecDeque::from([answer.map(str::to_owned)])),
        seen: Mutex::new(Vec::new()),
    }
}

fn block(command: &str, exit_code: i32, output: &str) -> TerminalBlockDto {
    TerminalBlockDto {
        command: command.into(),
        exit_code,
        output: output.into(),
    }
}

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc
        .with_ymd_and_hms(2026, 11, 20, 14, 3, 0)
        .unwrap()
}

fn input(conversation: &str, blocks: Vec<TerminalBlockDto>) -> CaptureInput {
    CaptureInput {
        conversation: conversation.to_owned(),
        blocks,
        working_directory: "file:///etc/nginx/".to_owned(),
        shell: Some("bash".to_owned()),
        host: Some("web-01".to_owned()),
        title_hint: None,
        now: now(),
    }
}

const ANSWER: &str = "Title: Replace nginx TLS certificate\nTags: nginx, TLS, #certbot, nginx\n\n\
## Goal\nInstall a new certificate.\n\n## Steps\n1. Copy it:\n```bash\nsudo cp fullchain.pem /etc/nginx/\n```\n\n\
## Verify\n```bash\nsudo nginx -t\n```\n\n## Pitfalls\n- `nginx -t` failed until the key matched.\n";

#[tokio::test]
async fn a_session_becomes_a_runbook_with_front_matter_and_a_slug() {
    let model = fake(Ok(ANSWER));
    let draft = draft_note(
        &model,
        input(
            "Person: how do I replace the cert?\n\nAssistant: copy it\n\n",
            vec![
                block("sudo cp fullchain.pem /etc/nginx/", 0, ""),
                block("sudo nginx -t", 1, "key values mismatch"),
                block("sudo nginx -t", 0, "syntax is ok"),
            ],
        ),
    )
    .await
    .unwrap();

    assert_eq!(draft.title, "Replace nginx TLS certificate");
    assert_eq!(draft.file_name, "replace-nginx-tls-certificate.md");
    assert_eq!(draft.tags, ["nginx", "tls", "certbot"]);
    assert_eq!(draft.redactions, 0);
    let (meta, body) = split_note(&draft.content);
    let meta = meta.expect("front matter");
    assert_eq!(meta.title.as_deref(), Some("Replace nginx TLS certificate"));
    assert_eq!(meta.created.as_deref(), Some("2026-11-20T14:03:00Z"));
    assert_eq!(meta.updated, meta.created);
    assert_eq!(meta.tags, ["nginx", "tls", "certbot"]);
    assert_eq!(meta.hosts, ["web-01"]);
    assert_eq!(meta.shell.as_deref(), Some("bash"));
    assert_eq!(meta.source.as_deref(), Some("terminal-session"));
    assert!(body.starts_with("# Replace nginx TLS certificate\n\n## Goal\n"));
    assert!(body.contains("## Pitfalls"));

    let seen = model.seen.lock().unwrap();
    let (role, generation) = &seen[0];
    assert_eq!(*role, LlmRole::Chat);
    assert_eq!(generation.system_prompt, CAPTURE_PROMPT);
    let prompt = &generation.user_prompt;
    assert!(prompt.contains("Shell: bash"));
    assert!(prompt.contains("Machine: web-01"));
    assert!(prompt.contains("Working directory: /etc/nginx/"));
    assert!(prompt.contains("Person: how do I replace the cert?"));
    assert!(prompt.contains("$ sudo nginx -t\n(exit status 1)\nkey values mismatch"));
}

#[tokio::test]
async fn secrets_never_reach_the_model_or_the_note() {
    let echoed = "Title: Deploy\nTags: deploy\n\n## Steps\n```bash\nexport API_TOKEN=abc123secret\n```\n\
-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg\n-----END PRIVATE KEY-----\n";
    let model = fake(Ok(echoed));
    let draft = draft_note(
        &model,
        input(
            "Person: my password=hunter2 works?\n\n",
            vec![block(
                "curl -H 'Authorization: Bearer sk-live-123456' https://x",
                0,
                "-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\nTOKEN=zzz",
            )],
        ),
    )
    .await
    .unwrap();

    let prompt = model.seen.lock().unwrap()[0].1.user_prompt.clone();
    for secret in ["hunter2", "sk-live-123456", "AAAA", "zzz"] {
        assert!(!prompt.contains(secret), "{secret} reached the model");
    }
    for secret in ["abc123secret", "MIIEvQIBADANBg"] {
        assert!(!draft.content.contains(secret), "{secret} reached the note");
    }
    assert_eq!(draft.redactions, 2);
}

#[tokio::test]
async fn front_matter_and_fenced_replies_are_understood_and_a_hint_names_the_note() {
    let fenced = "```markdown\n---\ntitle: Create a CSR\ntags: [openssl]\n---\n# Create a CSR\n\n## Steps\nrun openssl\n```";
    let model = fake(Ok(fenced));
    let draft = draft_note(&model, input("", vec![block("openssl req -new", 0, "")]))
        .await
        .unwrap();
    assert_eq!(draft.title, "Create a CSR");
    assert_eq!(draft.tags, ["openssl"]);
    let (_, body) = split_note(&draft.content);
    assert!(body.starts_with("# Create a CSR\n\n## Steps\nrun openssl"));
    assert!(!draft.content.contains("```markdown"));

    let model = fake(Ok("Just some steps without a title."));
    let mut hinted = input("", vec![block("ls", 0, "")]);
    hinted.title_hint = Some("  My CSR notes ".into());
    let draft = draft_note(&model, hinted).await.unwrap();
    assert_eq!(draft.title, "My CSR notes");
    assert_eq!(draft.file_name, "my-csr-notes.md");
    let (_, body) = split_note(&draft.content);
    assert!(body.starts_with("# My CSR notes\n\nJust some steps"));

    let model = fake(Ok("No title here either."));
    let draft = draft_note(&model, input("", vec![block("ls", 0, "")]))
        .await
        .unwrap();
    assert_eq!(draft.title, "Terminal session");
}

#[tokio::test]
async fn long_output_is_bounded_and_the_newest_commands_are_kept() {
    let model = fake(Ok(ANSWER));
    let mut blocks: Vec<TerminalBlockDto> = (0..80)
        .map(|n| block(&format!("step-{n}"), 0, &"x".repeat(4000)))
        .collect();
    blocks.push(block("final-command", 0, "done"));
    draft_note(&model, input(&"talk ".repeat(20_000), blocks))
        .await
        .unwrap();
    let prompt = model.seen.lock().unwrap()[0].1.user_prompt.clone();
    assert!(prompt.len() < MAX_PROMPT_BYTES + 2048, "{}", prompt.len());
    assert!(prompt.contains("$ final-command"));
    assert!(!prompt.contains("$ step-0\n"));
}

#[tokio::test]
async fn nothing_to_capture_and_model_failures_are_reported() {
    let model = fake(Ok(ANSWER));
    assert_eq!(
        draft_note(&model, input("   ", Vec::new())).await,
        Err(ChatFailureDto::InvalidRequest)
    );
    assert!(model.seen.lock().unwrap().is_empty());

    let consent = fake(Err(LlmProfileError::ConsentRequired(
        "api.example.test".into(),
    )));
    assert_eq!(
        draft_note(&consent, input("Person: hi\n\n", Vec::new())).await,
        Err(ChatFailureDto::ConsentRequired)
    );
    let blank = fake(Ok("  \n"));
    assert_eq!(
        draft_note(&blank, input("Person: hi\n\n", Vec::new())).await,
        Err(ChatFailureDto::Empty)
    );
}

#[tokio::test]
async fn a_capture_carries_a_use_when_description() {
    let described = "Title: Replace cert\nTags: nginx\nDescription: Use when an nginx certificate expires\n\n## Steps\n1. go\n";
    let draft = draft_note(&fake(Ok(described)), input("Person: x\n\n", vec![]))
        .await
        .unwrap();
    let meta = split_note(&draft.content).0.unwrap();
    assert_eq!(
        meta.description.as_deref(),
        Some("Use when an nginx certificate expires")
    );

    // Without one from the model, the title makes a description that still reads like a skill.
    let draft = draft_note(&fake(Ok(ANSWER)), input("Person: x\n\n", vec![]))
        .await
        .unwrap();
    let meta = split_note(&draft.content).0.unwrap();
    assert_eq!(
        meta.description.as_deref(),
        Some("Use when you need to: Replace nginx TLS certificate")
    );
}
