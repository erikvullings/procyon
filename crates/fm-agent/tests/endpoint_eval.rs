//! Opt-in harness evaluation against OpenAI-compatible chat completions.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use async_trait::async_trait;
use fm_agent::{
    EditFile, EditRequest, FileTools, Message, ModelProfile, NoteProposal, SkillCatalogue,
    ToolCallRecord, ToolCalling, ToolContext, ToolOutput, TurnEnd, TurnHost, TurnOptions, Typed,
    compose_system_prompt, match_edit, read_only_registry, repair_tool_arguments, run_turn,
    tool_call_line,
};
use serde_json::{Value, json};

enum Approval {
    Edit(EditRequest),
    Note,
}

impl From<EditRequest> for Approval {
    fn from(request: EditRequest) -> Self {
        Self::Edit(request)
    }
}

impl From<NoteProposal> for Approval {
    fn from(_: NoteProposal) -> Self {
        Self::Note
    }
}

struct Fixture(PathBuf);

#[async_trait]
impl FileTools for Fixture {
    async fn list_dir(&self, _: &str) -> ToolOutput {
        ToolOutput::new("List", "fixture.txt")
    }

    async fn read_file(&self, path: &str, _: usize) -> ToolOutput {
        if path != "fixture.txt" {
            return ToolOutput::error("Read", "file not found; use fixture.txt");
        }
        match std::fs::read_to_string(self.0.join("fixture.txt")) {
            Ok(text) => ToolOutput::new("Read", text),
            Err(error) => ToolOutput::error("Read", error),
        }
    }

    async fn grep(&self, _: &str, _: Option<&str>) -> ToolOutput {
        ToolOutput::error(
            "Search",
            "grep is unavailable for this fixture; use read_file",
        )
    }
}

struct Endpoint<'a> {
    url: &'a str,
    api_key: &'a str,
    model: &'a str,
    dir: &'a Path,
    body: Value,
    messages: Vec<Value>,
    rounds: usize,
    failed: BTreeMap<&'static str, usize>,
}

#[async_trait]
impl TurnHost for Endpoint<'_> {
    type Error = String;

    async fn generate(
        &mut self,
        _: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, String> {
        self.rounds += 1;
        self.body["messages"] = json!(self.messages);
        let body_path = self.dir.join("request.json");
        std::fs::write(&body_path, self.body.to_string()).map_err(|error| error.to_string())?;
        let mut child = Command::new("curl")
            .args([
                "-q",
                "--fail",
                "--silent",
                "--show-error",
                "--max-time",
                "90",
                "--config",
                "-",
            ])
            .args([
                "--header",
                "Content-Type: application/json",
                "--data-binary",
            ])
            .arg(format!("@{}", body_path.display()))
            .arg("--")
            .arg(self.url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        if !self.api_key.is_empty() {
            let key = self.api_key.replace('\\', "\\\\").replace('"', "\\\"");
            writeln!(
                child.stdin.take().expect("configured stdin"),
                "header = \"Authorization: Bearer {key}\""
            )
            .map_err(|error| error.to_string())?;
        }
        let response = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !response.status.success() {
            return Err(format!("curl exited with {}", response.status));
        }
        let response: Value =
            serde_json::from_slice(&response.stdout).map_err(|error| error.to_string())?;
        let message = response
            .pointer("/choices/0/message")
            .ok_or("missing completion message")?;
        let mut reply = message["content"].as_str().unwrap_or_default().to_owned();
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            let function = &call["function"];
            let name = function["name"].as_str().ok_or("missing tool name")?;
            let arguments = function["arguments"].as_str().unwrap_or_default();
            let args = repair_tool_arguments(arguments).unwrap_or_else(|| json!(arguments));
            reply.push('\n');
            reply.push_str(&tool_call_line(call["id"].as_str(), name, &args));
        }
        Ok(reply)
    }

    fn push_assistant(&mut self, text: &str, calls: &[ToolCallRecord]) {
        let mut message = json!({"role": "assistant", "content": Message::prose(text)});
        if !calls.is_empty() {
            message["tool_calls"] = json!(
                calls
                    .iter()
                    .map(|call| json!({
                        "id": call.id, "type": "function",
                        "function": {"name": call.name, "arguments": call.arguments.to_string()}
                    }))
                    .collect::<Vec<_>>()
            );
        }
        self.messages.push(message);
    }

    fn push_result(&mut self, call: &ToolCallRecord, text: &str) {
        let cause = if text.starts_with("Error: invalid arguments") {
            Some("invalid_arguments")
        } else if text.starts_with("Error: unknown tool") {
            Some("unknown_tool")
        } else if text.starts_with("Error:") {
            Some("tool_error")
        } else if text == fm_agent::NOT_RUN_RESULT {
            Some("skipped")
        } else {
            None
        };
        if let Some(cause) = cause {
            *self.failed.entry(cause).or_default() += 1;
        }
        self.messages
            .push(json!({"role": "tool", "tool_call_id": call.id, "content": text}));
    }

    fn push_note(&mut self, text: &str) {
        self.messages.push(json!({"role": "user", "content": text}));
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

#[tokio::test]
#[ignore = "requires an explicitly configured real model endpoint"]
async fn endpoint_eval() {
    let url = std::env::var("FM_AGENT_EVAL_URL")
        .expect("set FM_AGENT_EVAL_URL to the full chat completions URL");
    let models = std::env::var("FM_AGENT_EVAL_MODELS")
        .expect("set FM_AGENT_EVAL_MODELS to a comma-separated list");
    let key = std::env::var("FM_AGENT_EVAL_API_KEY").unwrap_or_default();
    assert!(!key.contains(['\n', '\r']), "invalid API key");
    let mut registry = read_only_registry::<Approval>();
    let remove: Vec<_> = registry
        .names()
        .filter(|name| *name != "read_file")
        .collect();
    for name in remove {
        registry.remove(name);
    }
    registry.register(Typed(EditFile));
    let tools = registry.specs().into_iter().map(|spec| json!({
        "type": "function",
        "function": {"name": spec.name, "description": spec.description, "parameters": spec.schema}
    })).collect::<Vec<_>>();
    let skills = SkillCatalogue::default();
    assert!(
        models.split(',').any(|model| !model.trim().is_empty()),
        "no models configured"
    );
    let mut endpoint_available = true;
    for model in models
        .split(',')
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        let profile = ModelProfile::for_model(model);
        let options = TurnOptions {
            profile,
            max_steps: 8,
            max_result_bytes: Some(2048),
            ..TurnOptions::default()
        };
        let mut total_rounds = 0;
        let mut successes = 0;
        let mut failures = BTreeMap::new();
        for (name, question, content, expected) in [
            (
                "read",
                "Read fixture.txt and reply with just its code.",
                "SUN-731".to_owned(),
                "SUN-731",
            ),
            (
                "bounded_read",
                "Read fixture.txt once and reply with just the code on its first line.",
                format!("MOON-619\n{}", "filler\n".repeat(600)),
                "MOON-619",
            ),
            (
                "edit",
                "Use edit_file to replace orange with blue in fixture.txt. Preserve the rest of the file.",
                "palette:\n    orange\n    green\n".to_owned(),
                "palette:\n    blue\n    green\n",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let files = Fixture(dir.path().to_owned());
            let path = dir.path().join("fixture.txt");
            std::fs::write(&path, &content).unwrap();
            let context = ToolContext {
                files: Some(&files),
                ..ToolContext::new(&skills)
            };
            let prompt = compose_system_prompt(
                "Complete the requested task using the provided tools.",
                &registry,
                ToolCalling::Native,
                &skills,
                question,
            );
            let mut host = Endpoint {
                url: &url,
                api_key: &key,
                model,
                dir: dir.path(),
                body: json!({"model": model, "tools": tools}),
                messages: vec![
                    json!({"role": "system", "content": prompt}),
                    json!({"role": "user", "content": question}),
                ],
                rounds: 0,
                failed: BTreeMap::new(),
            };
            let success = loop {
                let remaining = options.max_steps.saturating_sub(host.rounds);
                if remaining == 0 {
                    break false;
                }
                let remaining_options = TurnOptions {
                    max_steps: remaining,
                    ..options.clone()
                };
                let end = run_turn(
                    &mut host,
                    &registry,
                    &context,
                    &remaining_options,
                    &mut |_| {},
                )
                .await;
                match end {
                    Ok(TurnEnd::Answer(answer)) if name != "edit" => {
                        break answer.trim() == expected
                            && host.messages.iter().any(|message| {
                                message["role"] == "tool"
                                    && message["content"]
                                        .as_str()
                                        .is_some_and(|text| text.starts_with(expected))
                            });
                    }
                    Ok(TurnEnd::Answer(_)) => {
                        break std::fs::read_to_string(&path).unwrap() == expected;
                    }
                    Ok(TurnEnd::Interrupted {
                        call,
                        interrupt: Approval::Edit(request),
                    }) => {
                        if name != "edit" || request.path != "fixture.txt" {
                            *host.failed.entry("edit_scope").or_default() += 1;
                            host.push_result(&call, "Error: edits are allowed only on fixture.txt during the edit task.");
                            continue;
                        }
                        let current = std::fs::read_to_string(&path).unwrap();
                        match match_edit(&current, &request) {
                            Ok((edited, tier)) => {
                                std::fs::write(&path, edited).unwrap();
                                host.push_result(
                                    &call,
                                    &format!("Edited fixture.txt (matched {tier})."),
                                );
                            }
                            Err(error) => {
                                *host.failed.entry("edit_match").or_default() += 1;
                                host.push_result(
                                    &call,
                                    &format!("Could not edit fixture.txt: {error}"),
                                );
                            }
                        }
                    }
                    Ok(TurnEnd::LoopStopped(_)) => {
                        *host.failed.entry("repeat_limit").or_default() += 1;
                        break false;
                    }
                    Err(error) => {
                        eprintln!("{} {name}: {error}", host.model);
                        endpoint_available = false;
                        break false;
                    }
                    _ => break false,
                }
            };
            total_rounds += host.rounds;
            successes += usize::from(success);
            println!(
                "{model} {name} success={success} rounds={} failed_calls={:?}",
                host.rounds, host.failed
            );
            for (cause, count) in host.failed {
                *failures.entry(cause).or_insert(0) += count;
            }
        }
        println!("{model} success={successes}/3 rounds={total_rounds} failed_calls={failures:?}");
    }
    assert!(endpoint_available, "one or more endpoint requests failed");
}
