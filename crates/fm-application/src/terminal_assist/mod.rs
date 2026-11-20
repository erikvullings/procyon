//! Terminal assistance (tasks 0333 and 0336): answers about the embedded terminal through the chat
//! role (0322). `command`, `explain` and `fix` are one-shot. `ask` runs in a per-tab session with
//! the person's chosen context and may use read-only tools; a command it wants to run is only
//! proposed, and runs after the person approves it. Nothing is saved to disk.

mod context;
mod protocol;
mod redact;
mod session;
mod tools;

pub(crate) use session::Sessions;

use fm_transport_dto::{
    ChatFailureDto, TerminalAssistKindDto, TerminalAssistReplyDto, TerminalAssistRequestDto,
    TerminalToolResultDto,
};
use tokio_util::sync::CancellationToken;

use crate::chat::ChatGenerator;
use crate::file_assistant::truncate_to_bytes;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole};

use self::context::{AttachedFile, path_from_location, render_context};
use self::protocol::{ToolCall, parse_tool_call};
use self::redact::redact_secrets;
use self::tools::{Sandbox, ToolOutput};

/// Longest natural-language request, in characters.
const MAX_REQUEST_CHARS: usize = 2 * 1024;
/// Most transcript bytes sent; the newest output is kept.
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
/// Longest answer kept, in bytes.
const MAX_REPLY_BYTES: usize = 8 * 1024;
/// Upper bound on generated tokens for one answer.
const MAX_REPLY_TOKENS: u32 = 1024;
/// Most tool steps (read-only tools and approved commands) for one question.
const MAX_STEPS: u32 = 8;
/// Most files attached with `@path` to one question.
const MAX_ATTACHED_FILES: usize = 5;
/// Longest session id, in characters.
const MAX_SESSION_ID_CHARS: usize = 64;
/// Most output bytes of an approved command shown to the model.
const MAX_COMMAND_OUTPUT_BYTES: usize = 8 * 1024;

const ASK_PROMPT: &str = "You are the assistant in the person's embedded terminal. The context \
     shows their system, folder, selection and recent commands. Answer briefly and use only what \
     you can see or look up. To suggest a command for the person to review, write it on its own \
     final line as `Command: <command>`. Prefer read-only commands and never chain several \
     destructive steps.\n\
     You may use tools. To use one, reply with exactly one line and nothing else: `TOOL {json}`. \
     Then wait for the result. Tools: \
     {\"tool\":\"list_dir\",\"path\":\".\"} lists a folder; \
     {\"tool\":\"read_file\",\"path\":\"Cargo.toml\"} reads a text file; \
     {\"tool\":\"grep\",\"pattern\":\"text\",\"path\":\"src\"} finds lines containing text \
     (path is optional); \
     {\"tool\":\"run_command\",\"command\":\"cargo test\"} asks the person to run a command in \
     their terminal. They may decline, and the result returns with its exit status and output. \
     Paths are relative to the working folder and cannot leave it. Look things up instead of \
     guessing.";

const COMMAND_PROMPT: &str = "You turn a request into one command for the person's shell. Reply \
     with exactly one line containing only the command: no explanation, no markdown, no code \
     fences. Prefer read-only commands when they satisfy the request, and never chain several \
     destructive steps.";
const EXPLAIN_PROMPT: &str = "You explain recent terminal output for the person. Say what happened \
     in a few short sentences, and whether it is an error. Use only what the output shows, and say \
     when it does not tell you enough.";
const FIX_PROMPT: &str = "The recent terminal output ends with a failed command. Explain the most \
     likely cause in one or two sentences. Then, on its own final line, write `Command: ` followed \
     by one corrected shell command. If no safe command is clear, leave out that line.";

/// Answers one terminal assistance request through the chat role.
pub(crate) async fn reply(
    generator: &impl ChatGenerator,
    sessions: &Sessions,
    request: TerminalAssistRequestDto,
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    if request.kind == TerminalAssistKindDto::Ask {
        return ask(generator, sessions, request).await;
    }
    let (system_prompt, user_prompt) = prompts(&request)?;
    let raw = generate(generator, system_prompt, user_prompt).await?;
    let text = match request.kind {
        TerminalAssistKindDto::Command => first_command_line(&raw),
        _ => {
            let trimmed = raw.trim();
            (!trimmed.is_empty()).then(|| truncate_to_bytes(trimmed, MAX_REPLY_BYTES).to_owned())
        }
    }
    .ok_or(ChatFailureDto::Empty)?;
    Ok(TerminalAssistReplyDto {
        text,
        steps: Vec::new(),
        run_command: None,
    })
}

async fn generate(
    generator: &impl ChatGenerator,
    system_prompt: String,
    user_prompt: String,
) -> Result<String, ChatFailureDto> {
    let cancellation = CancellationToken::new();
    generator
        .generate_for_role(
            LlmRole::Chat,
            LlmChatGeneration {
                system_prompt,
                user_prompt,
                images: Vec::new(),
                maximum_tokens: MAX_REPLY_TOKENS,
                context_window: None,
                temperature: None,
            },
            &cancellation,
            &mut |_: &str| {},
        )
        .await
        .map_err(failure)
}

/// A session question: remembers the turn, lets the model use read-only tools, and stops at a
/// command that needs the person's approval.
async fn ask(
    generator: &impl ChatGenerator,
    sessions: &Sessions,
    request: TerminalAssistRequestDto,
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    let id = request
        .session_id
        .as_deref()
        .filter(|id| {
            !id.is_empty()
                && id.chars().count() <= MAX_SESSION_ID_CHARS
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
        })
        .ok_or(ChatFailureDto::InvalidRequest)?
        .to_owned();
    let sandbox =
        path_from_location(&request.working_directory).and_then(|path| Sandbox::new(&path));
    let mut steps = Vec::new();

    if let Some(result) = &request.tool_result {
        if sessions.render(&id).is_empty() {
            return Err(ChatFailureDto::InvalidRequest);
        }
        if result.declined {
            sessions.push_tool(
                &id,
                &format!(
                    "The person declined to run `{}`. Stop and wait for their next message.",
                    redact_secrets(&result.command)
                ),
            );
            return Ok(stopped(String::new(), steps));
        }
        sessions.push_tool(&id, &command_result_text(result));
        if sessions.next_step(&id) > MAX_STEPS {
            return Ok(stopped(step_limit_text(), steps));
        }
    } else {
        let question = request
            .request
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.chars().count() <= MAX_REQUEST_CHARS)
            .ok_or(ChatFailureDto::InvalidRequest)?;
        let files = attached_files(sandbox.as_ref(), &request.context.files);
        sessions.push_person(&id, question, &render_context(&request, &files));
    }

    loop {
        let raw = generate(generator, ASK_PROMPT.to_owned(), sessions.render(&id)).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ChatFailureDto::Empty);
        }
        let trimmed = truncate_to_bytes(trimmed, MAX_REPLY_BYTES);
        sessions.push_assistant(&id, trimmed);
        match parse_tool_call(trimmed) {
            None => return Ok(stopped(trimmed.to_owned(), steps)),
            Some(ToolCall::RunCommand { command }) => {
                return Ok(TerminalAssistReplyDto {
                    text: String::new(),
                    steps,
                    run_command: Some(command.trim().to_owned()),
                });
            }
            Some(call) => {
                if sessions.next_step(&id) > MAX_STEPS {
                    return Ok(stopped(step_limit_text(), steps));
                }
                let output = match sandbox.clone() {
                    Some(sandbox) => tokio::task::spawn_blocking(move || sandbox.run(&call))
                        .await
                        .map_err(|_| ChatFailureDto::Unavailable)?,
                    None => ToolOutput {
                        step: "Looked up a file".to_owned(),
                        text: "Error: file tools are not available for this location.".to_owned(),
                    },
                };
                steps.push(output.step);
                sessions.push_tool(&id, &output.text);
            }
        }
    }
}

fn stopped(text: String, steps: Vec<String>) -> TerminalAssistReplyDto {
    TerminalAssistReplyDto {
        text,
        steps,
        run_command: None,
    }
}

fn step_limit_text() -> String {
    format!("Stopped after {MAX_STEPS} steps. Ask again to continue.")
}

fn command_result_text(result: &TerminalToolResultDto) -> String {
    let status = result
        .exit_code
        .map_or_else(|| "unknown".to_owned(), |code| code.to_string());
    let output = redact_secrets(newest_bytes(
        result.output.trim_end(),
        MAX_COMMAND_OUTPUT_BYTES,
    ));
    format!(
        "The person ran `{}`. Exit status {status}. Output:\n{output}",
        redact_secrets(&result.command)
    )
}

/// The files the person named with `@path`, read inside the working folder.
fn attached_files(sandbox: Option<&Sandbox>, names: &[String]) -> Vec<AttachedFile> {
    let Some(sandbox) = sandbox else {
        return Vec::new();
    };
    names
        .iter()
        .take(MAX_ATTACHED_FILES)
        .filter_map(|name| {
            sandbox.read_text(name).map(|text| AttachedFile {
                name: name.clone(),
                text,
            })
        })
        .collect()
}

fn prompts(request: &TerminalAssistRequestDto) -> Result<(String, String), ChatFailureDto> {
    let context = render_context(request, &[]);
    match request.kind {
        TerminalAssistKindDto::Command => {
            let wanted = request
                .request
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty() && text.chars().count() <= MAX_REQUEST_CHARS)
                .ok_or(ChatFailureDto::InvalidRequest)?;
            Ok((
                COMMAND_PROMPT.to_owned(),
                format!("{context}\nRequest: {wanted}"),
            ))
        }
        TerminalAssistKindDto::Ask => Err(ChatFailureDto::InvalidRequest),
        TerminalAssistKindDto::Explain | TerminalAssistKindDto::Fix => {
            let transcript = request.transcript.trim();
            if transcript.is_empty() {
                return Err(ChatFailureDto::InvalidRequest);
            }
            let system = match request.kind {
                TerminalAssistKindDto::Fix => FIX_PROMPT,
                _ => EXPLAIN_PROMPT,
            };
            Ok((
                system.to_owned(),
                format!(
                    "{context}\nTerminal output:\n{}",
                    newest_bytes(transcript, MAX_TRANSCRIPT_BYTES)
                ),
            ))
        }
    }
}

/// The last `max_bytes` of `text`, starting on a character boundary.
pub(super) fn newest_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The first non-empty line of a command answer, without code fences, prompts or backticks.
fn first_command_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(|line| {
            let line = line.strip_prefix("$ ").unwrap_or(line);
            line.trim().trim_matches('`').trim().to_owned()
        })
        .find(|line| !line.is_empty())
        .map(|line| truncate_to_bytes(&line, MAX_REPLY_BYTES).to_owned())
}

fn failure(error: LlmProfileError) -> ChatFailureDto {
    match error {
        LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
        LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
        LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
        _ => ChatFailureDto::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use fm_transport_dto::{TerminalContextDto, TerminalToolResultDto};

    use super::*;

    enum Outcome {
        Text(&'static str),
        Consent,
        NoRole,
    }

    struct FakeGenerator {
        outcome: Outcome,
        /// Answers handed out first, one per call, before `outcome` applies.
        script: StdMutex<VecDeque<&'static str>>,
        seen: StdMutex<Vec<(LlmRole, LlmChatGeneration)>>,
    }

    #[async_trait]
    impl ChatGenerator for FakeGenerator {
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
            if let Some(text) = self.script.lock().unwrap().pop_front() {
                return Ok(text.to_owned());
            }
            match self.outcome {
                Outcome::Text(text) => Ok(text.to_owned()),
                Outcome::Consent => {
                    Err(LlmProfileError::ConsentRequired("api.example.test".into()))
                }
                Outcome::NoRole => Err(LlmProfileError::RoleNotAssigned),
            }
        }
    }

    fn generator(outcome: Outcome) -> FakeGenerator {
        FakeGenerator {
            outcome,
            script: StdMutex::new(VecDeque::new()),
            seen: StdMutex::new(Vec::new()),
        }
    }

    fn scripted(answers: &[&'static str]) -> FakeGenerator {
        let fake = generator(Outcome::Text("unscripted"));
        fake.script.lock().unwrap().extend(answers.iter().copied());
        fake
    }

    fn reply(
        fake: &FakeGenerator,
        request: TerminalAssistRequestDto,
    ) -> impl std::future::Future<Output = Result<TerminalAssistReplyDto, ChatFailureDto>> + '_
    {
        let sessions = Sessions::default();
        async move { super::reply(fake, &sessions, request).await }
    }

    fn command(request: &str) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind: TerminalAssistKindDto::Command,
            transcript: String::new(),
            request: Some(request.to_owned()),
            working_directory: "file:///projects/app".to_owned(),
            shell: Some("zsh".to_owned()),
            session_id: None,
            context: TerminalContextDto::default(),
            tool_result: None,
        }
    }

    fn explain(transcript: &str, kind: TerminalAssistKindDto) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind,
            transcript: transcript.to_owned(),
            request: None,
            working_directory: "file:///projects/app".to_owned(),
            shell: None,
            session_id: None,
            context: TerminalContextDto::default(),
            tool_result: None,
        }
    }

    #[tokio::test]
    async fn a_command_answer_is_one_clean_line_asked_through_the_chat_role() {
        let fake = generator(Outcome::Text("```sh\n$ ls -la\n```"));
        let reply = reply(&fake, command("list files")).await.unwrap();

        assert_eq!(reply.text, "ls -la");
        let seen = fake.seen.lock().unwrap();
        let (role, generation) = &seen[0];
        assert_eq!(*role, LlmRole::Chat);
        assert!(generation.images.is_empty());
        assert!(generation.user_prompt.contains("Shell: zsh"));
        assert!(
            generation
                .user_prompt
                .contains("Working directory: /projects/app")
        );
        assert!(generation.user_prompt.ends_with("Request: list files"));
    }

    #[tokio::test]
    async fn a_command_without_a_request_never_reaches_the_model() {
        let fake = generator(Outcome::Text("ls"));
        let mut request = command("");
        request.request = None;

        assert_eq!(
            reply(&fake, request).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_explanation_keeps_the_newest_output_when_the_transcript_is_long() {
        let transcript = format!("{}END", "a".repeat(MAX_TRANSCRIPT_BYTES * 2));
        let fake = generator(Outcome::Text("It failed."));

        let reply = reply(&fake, explain(&transcript, TerminalAssistKindDto::Explain))
            .await
            .unwrap();

        assert_eq!(reply.text, "It failed.");
        let seen = fake.seen.lock().unwrap();
        let prompt = &seen[0].1.user_prompt;
        assert!(prompt.ends_with("END"));
        assert!(prompt.len() < MAX_TRANSCRIPT_BYTES + 512);
    }

    #[tokio::test]
    async fn explaining_needs_terminal_output() {
        let fake = generator(Outcome::Text("unused"));

        assert_eq!(
            reply(&fake, explain("   \n", TerminalAssistKindDto::Fix)).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_fix_keeps_the_explanation_and_its_command_line() {
        let answer = "The path does not exist.\nCommand: mkdir -p build";
        let fake = generator(Outcome::Text(answer));

        let reply = reply(
            &fake,
            explain("make: build: No such file", TerminalAssistKindDto::Fix),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, answer);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].1.system_prompt, FIX_PROMPT);
    }

    #[tokio::test]
    async fn an_empty_answer_is_an_empty_failure() {
        let blank = generator(Outcome::Text("   "));
        assert_eq!(
            reply(&blank, command("anything")).await,
            Err(ChatFailureDto::Empty)
        );

        let fences_only = generator(Outcome::Text("```\n```"));
        assert_eq!(
            reply(&fences_only, command("anything")).await,
            Err(ChatFailureDto::Empty)
        );
    }

    #[tokio::test]
    async fn consent_and_role_problems_are_reported_as_such() {
        assert_eq!(
            reply(&generator(Outcome::Consent), command("x")).await,
            Err(ChatFailureDto::ConsentRequired)
        );
        assert_eq!(
            reply(&generator(Outcome::NoRole), command("x")).await,
            Err(ChatFailureDto::RoleNotAssigned)
        );
    }

    // --- ask: sessions, tools and approval (task 0336) ---

    fn ask_request(
        session: &str,
        question: &str,
        directory: &std::path::Path,
    ) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind: TerminalAssistKindDto::Ask,
            transcript: String::new(),
            request: Some(question.to_owned()),
            working_directory: url::Url::from_directory_path(directory)
                .unwrap()
                .to_string(),
            shell: Some("zsh".to_owned()),
            session_id: Some(session.to_owned()),
            context: TerminalContextDto::default(),
            tool_result: None,
        }
    }

    async fn ask_in(
        fake: &FakeGenerator,
        sessions: &Sessions,
        request: TerminalAssistRequestDto,
    ) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
        super::reply(fake, sessions, request).await
    }

    #[tokio::test]
    async fn a_follow_up_in_the_same_session_sees_the_earlier_answer() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["It is a Rust project.", "Run cargo build."]);
        let sessions = Sessions::default();

        ask_in(
            &fake,
            &sessions,
            ask_request("t1", "what is this?", dir.path()),
        )
        .await
        .unwrap();
        let second = ask_in(
            &fake,
            &sessions,
            ask_request("t1", "how to build?", dir.path()),
        )
        .await
        .unwrap();

        assert_eq!(second.text, "Run cargo build.");
        let seen = fake.seen.lock().unwrap();
        let prompt = &seen[1].1.user_prompt;
        assert!(prompt.contains("Assistant: It is a Rust project."));
        assert!(prompt.ends_with("Person: how to build?\n\n"));
        assert_eq!(seen[1].1.system_prompt, ASK_PROMPT);
    }

    #[tokio::test]
    async fn another_session_does_not_see_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["first", "second"]);
        let sessions = Sessions::default();
        ask_in(
            &fake,
            &sessions,
            ask_request("t1", "private question", dir.path()),
        )
        .await
        .unwrap();
        ask_in(&fake, &sessions, ask_request("t2", "hello", dir.path()))
            .await
            .unwrap();
        assert!(
            !fake.seen.lock().unwrap()[1]
                .1
                .user_prompt
                .contains("private question")
        );
    }

    #[tokio::test]
    async fn the_model_can_read_a_file_before_answering() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha").unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"read_file","path":"a.txt"}"#,
            "It says alpha.",
        ]);
        let sessions = Sessions::default();

        let reply = ask_in(
            &fake,
            &sessions,
            ask_request("t1", "what is in a.txt?", dir.path()),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, "It says alpha.");
        assert_eq!(reply.steps, vec!["Read a.txt".to_owned()]);
        assert!(
            fake.seen.lock().unwrap()[1]
                .1
                .user_prompt
                .ends_with("Tool result:\nalpha\n\n")
        );
    }

    #[tokio::test]
    async fn a_command_waits_for_approval_and_its_result_continues_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"run_command","command":"cargo test"}"#,
            "The tests pass.",
        ]);
        let sessions = Sessions::default();

        let pending = ask_in(
            &fake,
            &sessions,
            ask_request("t1", "run the tests", dir.path()),
        )
        .await
        .unwrap();
        assert_eq!(pending.run_command.as_deref(), Some("cargo test"));
        assert_eq!(pending.text, "");

        let mut follow = ask_request("t1", "", dir.path());
        follow.request = None;
        follow.tool_result = Some(TerminalToolResultDto {
            command: "cargo test".into(),
            declined: false,
            exit_code: Some(0),
            output: "ok. 3 passed\nAPI_KEY=abc".into(),
        });
        let done = ask_in(&fake, &sessions, follow).await.unwrap();

        assert_eq!(done.text, "The tests pass.");
        let seen = fake.seen.lock().unwrap();
        let prompt = &seen[1].1.user_prompt;
        assert!(prompt.contains("Exit status 0. Output:\nok. 3 passed\nAPI_KEY=[redacted]"));
    }

    #[tokio::test]
    async fn declining_a_command_stops_without_asking_the_model_again() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[r#"TOOL {"tool":"run_command","command":"rm -rf build"}"#]);
        let sessions = Sessions::default();
        ask_in(&fake, &sessions, ask_request("t1", "clean", dir.path()))
            .await
            .unwrap();

        let mut decline = ask_request("t1", "", dir.path());
        decline.request = None;
        decline.tool_result = Some(TerminalToolResultDto {
            command: "rm -rf build".into(),
            declined: true,
            exit_code: None,
            output: String::new(),
        });
        let reply = ask_in(&fake, &sessions, decline).await.unwrap();

        assert_eq!(reply.text, "");
        assert_eq!(reply.run_command, None);
        assert_eq!(fake.seen.lock().unwrap().len(), 1);
        assert!(
            sessions
                .render("t1")
                .contains("declined to run `rm -rf build`")
        );
    }

    #[tokio::test]
    async fn too_many_tool_steps_end_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let looping = r#"TOOL {"tool":"list_dir","path":"."}"#;
        let fake = scripted(&[looping; 12]);
        let sessions = Sessions::default();

        let reply = ask_in(&fake, &sessions, ask_request("t1", "loop", dir.path()))
            .await
            .unwrap();

        assert!(reply.text.starts_with("Stopped after"));
        assert_eq!(fake.seen.lock().unwrap().len() as u32, MAX_STEPS + 1);
    }

    #[tokio::test]
    async fn named_files_are_attached_from_the_working_folder_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha").unwrap();
        let fake = scripted(&["ok"]);
        let sessions = Sessions::default();
        let mut request = ask_request("t1", "explain @a.txt", dir.path());
        request.context.files = vec!["a.txt".into(), "../outside".into()];

        ask_in(&fake, &sessions, request).await.unwrap();

        let prompt = fake.seen.lock().unwrap()[0].1.user_prompt.clone();
        assert!(prompt.contains("File a.txt:\nalpha"));
        assert!(!prompt.contains("outside"));
    }

    #[tokio::test]
    async fn an_ask_needs_a_valid_session_and_a_question() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["unused"]);
        let sessions = Sessions::default();

        let mut no_session = ask_request("t1", "q", dir.path());
        no_session.session_id = None;
        assert_eq!(
            ask_in(&fake, &sessions, no_session).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert_eq!(
            ask_in(&fake, &sessions, ask_request("bad id!", "q", dir.path())).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert_eq!(
            ask_in(&fake, &sessions, ask_request("t1", "  ", dir.path())).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        let mut orphan = ask_request("t9", "", dir.path());
        orphan.request = None;
        orphan.tool_result = Some(TerminalToolResultDto {
            command: "ls".into(),
            declined: false,
            exit_code: Some(0),
            output: String::new(),
        });
        assert_eq!(
            ask_in(&fake, &sessions, orphan).await,
            Err(ChatFailureDto::InvalidRequest)
        );
        assert!(fake.seen.lock().unwrap().is_empty());
    }
}
