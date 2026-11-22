//! Terminal assistance (tasks 0333 and 0336): answers about the embedded terminal through the chat
//! role (0322). `command`, `explain` and `fix` are one-shot. `ask` runs in a per-tab session with
//! the person's chosen context and may use read-only tools; a command it wants to run is only
//! proposed, and runs after the person approves it. Nothing is saved to disk.

mod context;
mod documents;
mod notes;
mod protocol;
mod redact;
mod remote;
mod session;
mod stream;
mod tools;
mod write;

pub(crate) use notes::{NoteToolCall, NoteToolOutcome, NotesAccess};
pub(crate) use redact::redact_secrets;
pub(crate) use session::Sessions;
pub(crate) use tools::{is_credential_folder, is_secret_name};

use fm_transport_dto::{
    ChatFailureDto, TerminalAssistEventDto, TerminalAssistKindDto, TerminalAssistReplyDto,
    TerminalAssistRequestDto, TerminalToolResultDto, TerminalWriteDecisionDto,
    TerminalWriteRequestDto,
};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use crate::chat::ChatGenerator;
use crate::file_assistant::truncate_to_bytes;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole};

use self::context::{AttachedFile, render_activity, render_context, state_sections};
use self::documents::{Documents, is_document};
use self::protocol::{ToolCall, parse_tool_call};
use self::session::PendingWrite;
use self::stream::LineStreamer;
use self::tools::{ToolHost, ToolOutput};
use self::write::{
    FileWriter, MAX_BYTES_PER_QUESTION, MAX_FILES_PER_QUESTION, OUTPUT_FOLDER, WriteError,
};

/// Longest natural-language request, in characters.
const MAX_REQUEST_CHARS: usize = 2 * 1024;
/// Most transcript bytes sent; the newest output is kept.
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
/// Longest answer kept, in bytes.
const MAX_REPLY_BYTES: usize = 8 * 1024;
/// Upper bound on generated tokens for one answer.
const MAX_REPLY_TOKENS: u32 = 1024;
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
     {\"tool\":\"read_file\",\"path\":\"Cargo.toml\"} reads a text file, or the text of an \
     EPUB, Word, PDF, PowerPoint or Excel document (a long file is cut \
     off with a note: continue with {\"tool\":\"read_file\",\"path\":\"Cargo.toml\",\"offset\":32768}); \
     {\"tool\":\"grep\",\"pattern\":\"text\",\"path\":\"src\"} finds lines containing text \
     (path is optional); \
     {\"tool\":\"run_command\",\"command\":\"cargo test\"} asks the person to run a command in \
     their terminal. They may decline, and the result returns with its exit status and output. \
     Paths are relative to the working folder and cannot leave it. Look things up instead of \
     guessing. Never say you will look something up without making the TOOL call in the same \
     reply.\n\
     The person keeps notes, such as runbooks for tasks they do rarely. Before answering a \
     how-to question, search them: \
     {\"tool\":\"search_notes\",\"query\":\"replace nginx certificate\"} finds notes; \
     {\"tool\":\"read_note\",\"path\":\"ops/nginx.md\"} reads one; \
     {\"tool\":\"propose_note\",\"title\":\"...\",\"content\":\"...\"} proposes a new \
     Markdown note and {\"tool\":\"propose_note_update\",\"path\":\"ops/nginx.md\",\
     \"content\":\"...\"} proposes new text for a note. The person reviews a proposal before \
     anything is saved; propose only when they ask for a note or a note is clearly outdated.";

/// Added to the prompt when the person allows the assistant to create files (task 0338).
const WRITE_PROMPT: &str = "\nYou can create new files: \
     {\"tool\":\"write_file\",\"path\":\"report.md\",\"content\":\"...\"} creates a Markdown \
     or text file in the folder summaries/ next to the working folder, after the person approves. \
     Use a short plain file name ending in .md or .txt, without folders. It only creates new \
     files and never changes an existing one: a taken name gets a number, and the result tells \
     you the final name. To summarise several documents, read one and then write its file, one \
     file per document, with a factual summary. Use it only when the person asks for files.";

fn ask_prompt(allow_file_writes: bool) -> String {
    if allow_file_writes {
        format!("{ASK_PROMPT}{WRITE_PROMPT}")
    } else {
        ASK_PROMPT.to_owned()
    }
}

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
    providers: &ProviderRegistry,
    sessions: &Sessions,
    notes: Option<&dyn NotesAccess>,
    request: TerminalAssistRequestDto,
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    reply_streaming(generator, providers, sessions, notes, request, &mut |_| {}).await
}

/// Like [`reply`], and for an `ask` also reports each finished line of the answer, and each step,
/// to `on_event` while the model is still writing.
pub(crate) async fn reply_streaming(
    generator: &impl ChatGenerator,
    providers: &ProviderRegistry,
    sessions: &Sessions,
    notes: Option<&dyn NotesAccess>,
    request: TerminalAssistRequestDto,
    on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    if request.kind == TerminalAssistKindDto::Ask {
        return ask(generator, providers, sessions, notes, request, on_event).await;
    }
    let (system_prompt, user_prompt) = prompts(&request)?;
    let raw = generate(generator, system_prompt, user_prompt, &mut |_: &str| {}).await?;
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
        note_proposal: None,
        write_request: None,
    })
}

async fn generate(
    generator: &impl ChatGenerator,
    system_prompt: String,
    user_prompt: String,
    on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
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
            on_delta,
        )
        .await
        .map_err(failure)
}

/// A session question: remembers the turn, lets the model use read-only tools, and stops at a
/// command that needs the person's approval.
async fn ask(
    generator: &impl ChatGenerator,
    providers: &ProviderRegistry,
    sessions: &Sessions,
    notes: Option<&dyn NotesAccess>,
    request: TerminalAssistRequestDto,
    on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
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
    let host = ToolHost::open(providers, &request.working_directory);
    let documents = Documents::new(providers);
    let mut steps = Vec::new();

    let folder_key = format!("{}|{OUTPUT_FOLDER}", request.working_directory);
    let writer = FileWriter::open(providers, &request.working_directory);
    if let Some(decision) = request.write_decision {
        let pending = sessions
            .take_write(&id)
            .ok_or(ChatFailureDto::InvalidRequest)?;
        if decision == TerminalWriteDecisionDto::Decline {
            sessions.push_tool(
                &id,
                "The person declined to create files. Do not try again. Stop and wait for their \
                 next message.",
            );
            return Ok(stopped(String::new(), steps));
        }
        sessions.grant_writes(
            &id,
            &folder_key,
            decision == TerminalWriteDecisionDto::Session,
        );
        create_file(
            writer.as_ref(),
            sessions,
            &id,
            pending,
            on_event,
            &mut steps,
        )
        .await;
        if sessions.is_cancelled(&id) {
            return Ok(stopped(String::new(), steps));
        }
    } else if let Some(result) = &request.tool_result {
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
        if sessions.is_cancelled(&id) {
            return Ok(stopped(String::new(), steps));
        }
    } else {
        let question = request
            .request
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty() && text.chars().count() <= MAX_REQUEST_CHARS)
            .ok_or(ChatFailureDto::InvalidRequest)?;
        let files = attached_files(host.as_ref(), &documents, &request.context.files).await;
        // The state goes in with the first question, and later only the parts that changed are
        // added: what was sent stays as it was, so a cached prompt prefix still matches and an
        // old command is never taken to have run in the new folder. Commands already sent in an
        // earlier turn are not sent again.
        let blocks = sessions.unsent_blocks(&id, &request.context.blocks);
        let state = sessions.state_update(&id, &state_sections(&request));
        let activity = render_activity(&blocks, &files);
        let context = [state, activity]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        sessions.push_person(&id, question, &context);
    }

    loop {
        let mut streamer = LineStreamer::new();
        let raw = generate(
            generator,
            ask_prompt(request.allow_file_writes),
            sessions.render(&id),
            &mut |delta: &str| {
                streamer.push(delta, &mut |text| {
                    on_event(TerminalAssistEventDto::Line {
                        text: text.to_owned(),
                    });
                });
            },
        )
        .await?;
        streamer.finish(&mut |text| {
            on_event(TerminalAssistEventDto::Line {
                text: text.to_owned(),
            });
        });
        // A reply that looked like a tool call but was not one is shown after all.
        if parse_tool_call(&raw).is_some() {
            streamer.discard_held();
        } else {
            streamer.release_held(&mut |text| {
                on_event(TerminalAssistEventDto::Line {
                    text: text.to_owned(),
                });
            });
        }
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
                    note_proposal: None,
                    write_request: None,
                });
            }
            Some(call) => {
                if sessions.is_cancelled(&id) {
                    return Ok(stopped(String::new(), steps));
                }
                if let ToolCall::WriteFile { path, content } = call {
                    let pending = PendingWrite {
                        name: path.trim().to_owned(),
                        content,
                    };
                    if !request.allow_file_writes {
                        sessions.push_tool(&id, "Error: creating files is turned off.");
                    } else if let Err(error) = write::check(&pending.name, &pending.content) {
                        sessions.push_tool(&id, &format!("Error: {error}"));
                    } else if sessions.writes_granted(&id, &folder_key) {
                        create_file(
                            writer.as_ref(),
                            sessions,
                            &id,
                            pending,
                            on_event,
                            &mut steps,
                        )
                        .await;
                    } else {
                        let asked = TerminalWriteRequestDto {
                            folder: format!("./{OUTPUT_FOLDER}/"),
                            name: pending.name.clone(),
                            bytes: u32::try_from(pending.content.len()).unwrap_or(u32::MAX),
                        };
                        sessions.hold_write(&id, pending);
                        return Ok(TerminalAssistReplyDto {
                            write_request: Some(asked),
                            ..stopped(String::new(), steps)
                        });
                    }
                    continue;
                }
                if let Some(note_call) = note_tool_call(&call) {
                    let outcome = match notes {
                        Some(notes) => notes.run(note_call).await,
                        None => NoteToolOutcome::Output {
                            step: "Looked in the notes".to_owned(),
                            text: "Error: notes are not available here.".to_owned(),
                        },
                    };
                    match outcome {
                        NoteToolOutcome::Proposal(proposal) => {
                            sessions.push_tool(
                                &id,
                                "The note was shown to the person for review. Nothing is saved \
                                 unless they save it.",
                            );
                            return Ok(TerminalAssistReplyDto {
                                text: String::new(),
                                steps,
                                run_command: None,
                                note_proposal: Some(proposal),
                                write_request: None,
                            });
                        }
                        NoteToolOutcome::Output { step, text } => {
                            on_event(TerminalAssistEventDto::Step { text: step.clone() });
                            steps.push(step);
                            sessions.push_tool(&id, &text);
                        }
                    }
                    continue;
                }
                let output = match &host {
                    Some(host) => match documents.run(host, &call).await {
                        Some(output) => output,
                        None => host.run(call).await,
                    },
                    None => ToolOutput {
                        step: "Looked up a file".to_owned(),
                        text: "Error: file tools are not available for this location.".to_owned(),
                    },
                };
                on_event(TerminalAssistEventDto::Step {
                    text: output.step.clone(),
                });
                steps.push(output.step);
                sessions.push_tool(&id, &output.text);
            }
        }
    }
}

/// Creates one approved file within the question's limits and tells the person and the model.
async fn create_file(
    writer: Option<&FileWriter>,
    sessions: &Sessions,
    id: &str,
    pending: PendingWrite,
    on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
    steps: &mut Vec<String>,
) {
    let result = if !sessions.reserve_write(
        id,
        pending.content.len(),
        MAX_FILES_PER_QUESTION,
        MAX_BYTES_PER_QUESTION,
    ) {
        Err(WriteError::QuestionLimit)
    } else {
        match writer {
            Some(writer) => writer.create(&pending.name, &pending.content).await,
            None => Err(WriteError::Folder),
        }
    };
    let (step, text) = match result {
        Ok(created) => (created.step(), created.result()),
        Err(error) => (
            format!("Could not create {}", pending.name),
            format!("Error: {error}"),
        ),
    };
    on_event(TerminalAssistEventDto::Step { text: step.clone() });
    steps.push(step);
    sessions.push_tool(id, &text);
}

/// The notes tool behind a parsed tool call, when it is one.
fn note_tool_call(call: &ToolCall) -> Option<NoteToolCall> {
    match call {
        ToolCall::SearchNotes { query, limit } => Some(NoteToolCall::Search {
            query: query.clone(),
            limit: *limit,
        }),
        ToolCall::ReadNote { path } => Some(NoteToolCall::Read { path: path.clone() }),
        ToolCall::ProposeNote { title, content } => Some(NoteToolCall::Propose {
            path: None,
            title: title.clone(),
            content: content.clone(),
        }),
        ToolCall::ProposeNoteUpdate { path, content } => Some(NoteToolCall::Propose {
            path: Some(path.clone()),
            title: None,
            content: content.clone(),
        }),
        _ => None,
    }
}

fn stopped(text: String, steps: Vec<String>) -> TerminalAssistReplyDto {
    TerminalAssistReplyDto {
        text,
        steps,
        run_command: None,
        note_proposal: None,
        write_request: None,
    }
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
async fn attached_files(
    host: Option<&ToolHost>,
    documents: &Documents,
    names: &[String],
) -> Vec<AttachedFile> {
    let Some(host) = host else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for name in names.iter().take(MAX_ATTACHED_FILES) {
        let text = if is_document(name) {
            documents.read(host, name, 0).await.ok()
        } else {
            host.read_text(name).await
        };
        if let Some(text) = text {
            files.push(AttachedFile {
                name: name.clone(),
                text,
            });
        }
    }
    files
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
            on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            self.seen.lock().unwrap().push((role, generation));
            if let Some(text) = self.script.lock().unwrap().pop_front() {
                // Hand the text over in small pieces, as a streaming endpoint does.
                let mut rest = text;
                while !rest.is_empty() {
                    let mut end = rest.len().min(5);
                    while !rest.is_char_boundary(end) {
                        end += 1;
                    }
                    on_delta(&rest[..end]);
                    rest = &rest[end..];
                }
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
        async move { super::reply(fake, &ProviderRegistry::new(), &sessions, None, request).await }
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
            write_decision: None,
            allow_file_writes: false,
            workspace_id: None,
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
            write_decision: None,
            allow_file_writes: false,
            workspace_id: None,
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
            write_decision: None,
            allow_file_writes: false,
            workspace_id: None,
        }
    }

    async fn ask_in(
        fake: &FakeGenerator,
        sessions: &Sessions,
        request: TerminalAssistRequestDto,
    ) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
        super::reply(fake, &ProviderRegistry::new(), sessions, None, request).await
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
    async fn a_long_tool_loop_runs_until_the_person_stops_it() {
        let dir = tempfile::tempdir().unwrap();
        let looping = r#"TOOL {"tool":"list_dir","path":"."}"#;
        let mut script = vec![looping; 20];
        script.push("done");
        let fake = scripted(&script);
        let sessions = Sessions::default();

        let reply = ask_in(&fake, &sessions, ask_request("t1", "loop", dir.path()))
            .await
            .unwrap();
        assert_eq!(reply.text, "done");
        assert_eq!(reply.steps.len(), 20);
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

    #[tokio::test]
    async fn a_follow_up_repeats_neither_the_state_nor_commands_already_sent() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["first", "second"]);
        let sessions = Sessions::default();
        let block = fm_transport_dto::TerminalBlockDto {
            command: "make".into(),
            exit_code: 2,
            output: "UNIQUE-OUTPUT".into(),
        };
        let mut one = ask_request("t1", "why?", dir.path());
        one.context.blocks = vec![block.clone()];
        ask_in(&fake, &sessions, one).await.unwrap();
        let mut two = ask_request("t1", "and now?", dir.path());
        two.context.blocks = vec![block];
        ask_in(&fake, &sessions, two).await.unwrap();

        let prompt = fake.seen.lock().unwrap()[1].1.user_prompt.clone();
        assert_eq!(prompt.matches("UNIQUE-OUTPUT").count(), 1);
        assert_eq!(prompt.matches("Working directory").count(), 1);
        assert!(prompt.starts_with("Context attached by the person:\nTerminal state:\n"));
        assert!(prompt.contains("Person: why?"));
        assert!(prompt.ends_with("Person: and now?\n\n"));
    }

    #[tokio::test]
    async fn a_new_folder_adds_its_state_and_keeps_the_old_one_in_place() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["first", "second"]);
        let sessions = Sessions::default();
        let mut one = ask_request("t1", "here?", first_dir.path());
        one.context.listing = vec!["old.txt".into()];
        ask_in(&fake, &sessions, one).await.unwrap();
        let mut two = ask_request("t1", "now?", second_dir.path());
        two.context.listing = vec!["new.txt".into()];
        ask_in(&fake, &sessions, two).await.unwrap();

        let seen = fake.seen.lock().unwrap();
        let first_prompt = &seen[0].1.user_prompt;
        let second_prompt = &seen[1].1.user_prompt;
        // What was sent before stays exactly as it was, so a cached prompt prefix still matches.
        assert!(second_prompt.starts_with(first_prompt.as_str()));
        assert!(second_prompt.contains("- old.txt"));
        assert!(second_prompt.contains("Terminal state changed:\n"));
        assert!(second_prompt.contains("- new.txt"));
        assert_eq!(second_prompt.matches("Working directory").count(), 2);
    }

    #[tokio::test]
    async fn the_answer_streams_line_by_line_with_steps_and_without_tool_calls() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            "Looking.\nTOOL {\"tool\":\"list_dir\",\"path\":\".\"}",
            "Done.\n- a\n- b",
        ]);
        let sessions = Sessions::default();
        let mut events = Vec::new();

        let reply = super::reply_streaming(
            &fake,
            &ProviderRegistry::new(),
            &sessions,
            None,
            ask_request("t1", "what is here?", dir.path()),
            &mut |event| events.push(event),
        )
        .await
        .unwrap();

        let line = |text: &str| TerminalAssistEventDto::Line { text: text.into() };
        assert_eq!(
            events,
            [
                line("Looking."),
                TerminalAssistEventDto::Step {
                    text: "Listed .".into()
                },
                line("Done."),
                line("- a"),
                line("- b"),
            ]
        );
        assert_eq!(reply.text, "Done.\n- a\n- b");
    }

    #[tokio::test]
    async fn a_held_line_that_was_not_a_tool_call_is_shown_at_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["Config:\n{\"name\": \"x\"}\nthat is all"]);
        let sessions = Sessions::default();
        let mut events = Vec::new();

        super::reply_streaming(
            &fake,
            &ProviderRegistry::new(),
            &sessions,
            None,
            ask_request("t1", "show", dir.path()),
            &mut |event| events.push(event),
        )
        .await
        .unwrap();

        let texts: Vec<String> = events
            .into_iter()
            .map(|event| match event {
                TerminalAssistEventDto::Line { text } | TerminalAssistEventDto::Step { text } => {
                    text
                }
            })
            .collect();
        assert_eq!(texts, ["Config:", "{\"name\": \"x\"}", "that is all"]);
    }

    // --- notes tools (task 0337) ---

    struct FakeNotes {
        calls: StdMutex<Vec<NoteToolCall>>,
    }

    #[async_trait]
    impl NotesAccess for FakeNotes {
        async fn run(&self, call: NoteToolCall) -> NoteToolOutcome {
            self.calls.lock().unwrap().push(call.clone());
            match call {
                NoteToolCall::Search { query, .. } => NoteToolOutcome::Output {
                    step: format!("Searched notes for {query}"),
                    text: "1. ops/nginx.md: Replace nginx certificate".into(),
                },
                NoteToolCall::Read { path } => NoteToolOutcome::Output {
                    step: format!("Read note {path}"),
                    text: "NOTE-BODY: sudo cp fullchain.pem /etc/nginx/".into(),
                },
                NoteToolCall::Propose { path, content, .. } => {
                    NoteToolOutcome::Proposal(fm_transport_dto::NoteProposalDto {
                        path,
                        draft: fm_transport_dto::NoteDraftDto {
                            title: "T".into(),
                            file_name: "t.md".into(),
                            tags: Vec::new(),
                            content,
                            redactions: 0,
                        },
                    })
                }
            }
        }
    }

    fn fake_notes() -> FakeNotes {
        FakeNotes {
            calls: StdMutex::new(Vec::new()),
        }
    }

    async fn ask_with_notes(
        fake: &FakeGenerator,
        sessions: &Sessions,
        notes: Option<&dyn NotesAccess>,
        request: TerminalAssistRequestDto,
    ) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
        super::reply(fake, &ProviderRegistry::new(), sessions, notes, request).await
    }

    #[tokio::test]
    async fn the_model_can_search_and_read_notes_before_answering() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"search_notes","query":"nginx certificate"}"#,
            r#"TOOL {"tool":"read_note","path":"ops/nginx.md"}"#,
            "Copy the chain as your note says.",
        ]);
        let notes = fake_notes();
        let sessions = Sessions::default();

        let reply = ask_with_notes(
            &fake,
            &sessions,
            Some(&notes),
            ask_request("t1", "how do I replace the nginx cert?", dir.path()),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, "Copy the chain as your note says.");
        assert_eq!(
            reply.steps,
            [
                "Searched notes for nginx certificate",
                "Read note ops/nginx.md"
            ]
        );
        assert_eq!(
            *notes.calls.lock().unwrap(),
            [
                NoteToolCall::Search {
                    query: "nginx certificate".into(),
                    limit: None
                },
                NoteToolCall::Read {
                    path: "ops/nginx.md".into()
                }
            ]
        );
        let seen = fake.seen.lock().unwrap();
        assert!(seen[0].1.system_prompt.contains("search_notes"));
        assert!(seen[2].1.user_prompt.contains("NOTE-BODY"));
    }

    #[tokio::test]
    async fn a_proposed_note_ends_the_turn_for_review() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            r##"TOOL {"tool":"propose_note_update","path":"ops/nginx.md","content":"# New"}"##,
            "never asked",
        ]);
        let notes = fake_notes();
        let sessions = Sessions::default();

        let reply = ask_with_notes(
            &fake,
            &sessions,
            Some(&notes),
            ask_request("t1", "update my nginx note", dir.path()),
        )
        .await
        .unwrap();

        let proposal = reply.note_proposal.expect("a proposal");
        assert_eq!(proposal.path.as_deref(), Some("ops/nginx.md"));
        assert_eq!(proposal.draft.content, "# New");
        assert_eq!(reply.text, "");
        assert_eq!(reply.run_command, None);
        assert_eq!(fake.seen.lock().unwrap().len(), 1);
        assert!(
            sessions
                .render("t1")
                .contains("shown to the person for review")
        );
    }

    #[tokio::test]
    async fn without_notes_the_model_is_told_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"search_notes","query":"csr"}"#,
            "I could not look at your notes.",
        ]);
        let sessions = Sessions::default();

        let reply = ask_with_notes(
            &fake,
            &sessions,
            None,
            ask_request("t1", "csr?", dir.path()),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, "I could not look at your notes.");
        assert!(
            fake.seen.lock().unwrap()[1]
                .1
                .user_prompt
                .contains("Error: notes are not available here.")
        );
    }

    #[test]
    fn notes_tool_calls_need_their_text() {
        use super::protocol::parse_tool_call;
        assert_eq!(
            parse_tool_call(r#"TOOL {"tool":"search_notes","query":"  "}"#),
            None
        );
        assert_eq!(
            parse_tool_call(r#"TOOL {"tool":"propose_note","content":""}"#),
            None
        );
        assert_eq!(
            parse_tool_call(r#"TOOL {"tool":"propose_note_update","path":" ","content":"x"}"#),
            None
        );
        assert_eq!(
            parse_tool_call(r#"TOOL {"tool":"search_notes","query":"tls","limit":3}"#),
            Some(ToolCall::SearchNotes {
                query: "tls".into(),
                limit: Some(3)
            })
        );
    }

    fn local_providers() -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.register(std::sync::Arc::new(
            fm_vfs_local::LocalFileSystemProvider::new(),
        ));
        registry
    }

    fn writing_request(
        session: &str,
        question: &str,
        directory: &std::path::Path,
    ) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            allow_file_writes: true,
            ..ask_request(session, question, directory)
        }
    }

    fn decision(
        session: &str,
        directory: &std::path::Path,
        decision: TerminalWriteDecisionDto,
    ) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            request: None,
            write_decision: Some(decision),
            ..writing_request(session, "", directory)
        }
    }

    async fn ask_local(
        fake: &FakeGenerator,
        sessions: &Sessions,
        request: TerminalAssistRequestDto,
    ) -> TerminalAssistReplyDto {
        super::reply(fake, &local_providers(), sessions, None, request)
            .await
            .unwrap()
    }

    const WRITE_A: &str = r##"TOOL {"tool":"write_file","path":"a.md","content":"# A"}"##;
    const WRITE_B: &str = r##"TOOL {"tool":"write_file","path":"b.md","content":"# B"}"##;

    #[tokio::test]
    async fn nothing_is_written_before_the_person_approves() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[WRITE_A]);
        let sessions = Sessions::default();
        let reply = ask_local(
            &fake,
            &sessions,
            writing_request("t", "summarise", dir.path()),
        )
        .await;
        let asked = reply.write_request.unwrap();
        assert_eq!(asked.folder, "./summaries/");
        assert_eq!(asked.name, "a.md");
        assert!(!dir.path().join("summaries").exists());
    }

    #[tokio::test]
    async fn one_approval_covers_the_later_files_of_the_question() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[WRITE_A, WRITE_B, "Done."]);
        let sessions = Sessions::default();
        ask_local(
            &fake,
            &sessions,
            writing_request("t", "summarise", dir.path()),
        )
        .await;
        let reply = ask_local(
            &fake,
            &sessions,
            decision("t", dir.path(), TerminalWriteDecisionDto::Question),
        )
        .await;
        assert_eq!(reply.text, "Done.");
        assert_eq!(
            reply.steps,
            vec![
                "Created summaries/a.md (3 B)",
                "Created summaries/b.md (3 B)"
            ]
        );
        assert!(dir.path().join("summaries/a.md").exists());
        assert!(dir.path().join("summaries/b.md").exists());
    }

    #[tokio::test]
    async fn a_question_grant_ends_with_the_question_but_a_session_grant_stays() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[WRITE_A, "ok", WRITE_B]);
        let sessions = Sessions::default();
        ask_local(&fake, &sessions, writing_request("t", "one", dir.path())).await;
        ask_local(
            &fake,
            &sessions,
            decision("t", dir.path(), TerminalWriteDecisionDto::Question),
        )
        .await;
        let next = ask_local(&fake, &sessions, writing_request("t", "two", dir.path())).await;
        assert!(next.write_request.is_some());

        let fake = scripted(&[WRITE_A, "ok", WRITE_B, "ok"]);
        let sessions = Sessions::default();
        ask_local(&fake, &sessions, writing_request("s", "one", dir.path())).await;
        ask_local(
            &fake,
            &sessions,
            decision("s", dir.path(), TerminalWriteDecisionDto::Session),
        )
        .await;
        let next = ask_local(&fake, &sessions, writing_request("s", "two", dir.path())).await;
        assert!(next.write_request.is_none());
        assert!(dir.path().join("summaries/b.md").exists());
    }

    #[tokio::test]
    async fn declining_writes_nothing_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[WRITE_A]);
        let sessions = Sessions::default();
        ask_local(&fake, &sessions, writing_request("t", "go", dir.path())).await;
        let reply = ask_local(
            &fake,
            &sessions,
            decision("t", dir.path(), TerminalWriteDecisionDto::Decline),
        )
        .await;
        assert_eq!(reply.text, "");
        assert!(reply.write_request.is_none());
        assert!(!dir.path().join("summaries").exists());
        assert!(sessions.render("t").contains("declined to create files"));
    }

    #[tokio::test]
    async fn an_answer_without_a_waiting_file_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[]);
        let sessions = Sessions::default();
        let result = super::reply(
            &fake,
            &local_providers(),
            &sessions,
            None,
            decision("t", dir.path(), TerminalWriteDecisionDto::Session),
        )
        .await;
        assert_eq!(result.unwrap_err(), ChatFailureDto::InvalidRequest);
    }

    #[tokio::test]
    async fn a_bad_name_is_refused_to_the_model_without_asking() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"write_file","path":"../../.zshrc","content":"x"}"#,
            "Cannot.",
        ]);
        let sessions = Sessions::default();
        let reply = ask_local(&fake, &sessions, writing_request("t", "go", dir.path())).await;
        assert!(reply.write_request.is_none());
        assert_eq!(reply.text, "Cannot.");
        assert!(sessions.render("t").contains("not an allowed file name"));
        assert!(!dir.path().join("summaries").exists());
    }

    #[tokio::test]
    async fn the_tool_is_unavailable_when_the_setting_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&[WRITE_A, "No files."]);
        let sessions = Sessions::default();
        let reply = ask_local(&fake, &sessions, ask_request("t", "go", dir.path())).await;
        assert!(reply.write_request.is_none());
        assert!(sessions.render("t").contains("turned off"));
        assert!(!ask_prompt(false).contains("write_file"));
        assert!(ask_prompt(true).contains("write_file"));
    }

    #[tokio::test]
    async fn the_file_count_per_question_is_limited() {
        let dir = tempfile::tempdir().unwrap();
        let mut script = vec![WRITE_A];
        let names: Vec<&'static str> = (0..MAX_FILES_PER_QUESTION)
            .map(|index| {
                &*Box::leak(
                    format!(r#"TOOL {{"tool":"write_file","path":"f{index}.md","content":"x"}}"#)
                        .into_boxed_str(),
                )
            })
            .collect();
        script.extend(names);
        script.push("Stopped.");
        let fake = scripted(&script);
        let sessions = Sessions::default();
        ask_local(&fake, &sessions, writing_request("t", "go", dir.path())).await;
        let reply = ask_local(
            &fake,
            &sessions,
            decision("t", dir.path(), TerminalWriteDecisionDto::Question),
        )
        .await;
        assert_eq!(reply.text, "Stopped.");
        let created = std::fs::read_dir(dir.path().join("summaries"))
            .unwrap()
            .count();
        assert_eq!(created, MAX_FILES_PER_QUESTION);
        assert!(sessions.render("t").contains("limit for this question"));
    }
}
