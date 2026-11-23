//! Terminal assistance (tasks 0333 and 0336): answers about the embedded terminal through the chat
//! role (0322). `command`, `explain` and `fix` are one-shot. `ask` runs in a per-tab session with
//! the person's chosen context and may use read-only tools; a command it wants to run is only
//! proposed, and runs after the person approves it. Nothing is saved to disk.

mod agent_tools;
mod commands;
mod context;
mod session;
mod write;

use std::sync::Arc;

use async_trait::async_trait;

use fm_agent::{
    Message, SkillIndex, ToolCallRecord, ToolCalling, ToolContext, ToolRegistry, ToolSpec, TurnEnd,
    TurnEvent, TurnHost, TurnOptions, compose_system_prompt, run_turn,
};

pub(crate) use fm_agent::{NotesAccess, redact_secrets};
pub(crate) use session::Sessions;

use fm_transport_dto::{
    ChatFailureDto, TerminalAssistEventDto, TerminalAssistFilesDto, TerminalAssistKindDto,
    TerminalAssistReplyDto, TerminalAssistRequestDto, TerminalAssistSkillDto,
    TerminalToolResultDto, TerminalWriteDecisionDto, TerminalWriteRequestDto,
};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use fm_agent::{newest_bytes, truncate_to_bytes};

use crate::agent_host::{
    Documents, FolderTools, ModelSideCalls, ProjectContext, SkillFolderDocuments, SkillScope,
    ToolHost, chat_failure, generation_parts, is_document, note_proposal_dto, project_context,
    tool_calling,
};
use crate::chat::ChatGenerator;
use crate::llm_profiles::{LlmChatGeneration, LlmRole, ReplyFormat};

use self::agent_tools::TerminalInterrupt;
use self::context::{AttachedFile, render_activity, render_context, state_sections};
use self::session::PendingWrite;
use self::write::{
    FileWriter, MAX_BYTES_PER_QUESTION, MAX_FILES_PER_QUESTION, OUTPUT_FOLDER, WriteError,
};

/// Longest one-shot answer (explain, fix) and longest `ask` answer kept, in bytes.
const MAX_REPLY_BYTES: usize = 8 * 1024;
/// Longest natural-language request, in characters.
const MAX_REQUEST_CHARS: usize = 2 * 1024;
/// Most transcript bytes sent; the newest output is kept.
const MAX_TRANSCRIPT_BYTES: usize = 16 * 1024;
/// No limit of its own on generated tokens: the profile's answer limit (set from the model's
/// own limits) is the only bound, so a long answer or a file's content is not cut short.
const MAX_REPLY_TOKENS: u32 = u32::MAX;
/// Most files attached with `@path` to one question.
const MAX_ATTACHED_FILES: usize = 5;
/// Longest session id, in characters.
const MAX_SESSION_ID_CHARS: usize = 64;
/// Most output bytes of an approved command shown to the model.
const MAX_COMMAND_OUTPUT_BYTES: usize = 8 * 1024;

const ASK_INTRO: &str = "You are the assistant in the person's embedded terminal. The context \
     shows their system, folder, selection and recent commands. Answer briefly and use only what \
     you can see or look up. A `Folder listing` in the context already names every file there: \
     use it, and never list the folder again. Look files up and read them with your tools; \
     never suggest a command (such as `ls` or `cat`) for something a tool can do. To suggest a \
     command for the person to review, write it on its own final line as `Command: <command>`. \
     Prefer read-only commands and never chain several destructive steps.";
/// How the model calls tools when the endpoint has no native tool calling.
const ASK_TEXT_TOOLS: &str = "\nYou may use tools. To use one, reply with exactly one line and \
     nothing else: `TOOL {json}`. Then wait for the result. Paths are relative to the working \
     folder and cannot leave it. Look things up instead of guessing. Never say you will look \
     something up without making the TOOL call in the same reply. Finish the whole task before \
     you answer: keep calling tools until every file you need is read and any file you were asked \
     to create is written. Never stop to report progress or to ask whether to continue; a reply \
     without a TOOL line ends the task.";
/// The same, when the endpoint takes the tools natively.
const ASK_NATIVE_TOOLS: &str = "\nYou may use the tools you are given, and call up to 8 in one reply (for example to read \
     several files at once). Paths \
     are relative to the working folder and cannot leave it. Look things up instead of guessing. \
     Never say you will look something up without calling the tool in the same reply. Finish \
     the whole task before you answer: keep calling tools until every file you need is read and \
     any file you were asked to create is written. Never stop to report progress or to ask \
     whether to continue; a reply without a tool call ends the task.";

/// The ask prompt: the fixed text, the project's instructions (task 0342), the tools generated
/// from the registry (task 0339), and the skills catalogue for this question.
fn ask_prompt(
    registry: &ToolRegistry<TerminalInterrupt>,
    project: &ProjectContext,
    request: &str,
    calling: ToolCalling,
) -> String {
    let mut intro = format!(
        "{ASK_INTRO}{}",
        match calling {
            ToolCalling::Native => ASK_NATIVE_TOOLS,
            ToolCalling::Text => ASK_TEXT_TOOLS,
        }
    );
    if !project.instructions.is_empty() {
        intro.push_str("\n\n");
        intro.push_str(&project.instructions);
    }
    compose_system_prompt(&intro, registry, calling, &project.skills, request)
}

/// How an `ask` turn runs: the answer cap, and secret redaction of every tool result.
fn turn_options() -> TurnOptions {
    TurnOptions {
        max_answer_bytes: Some(MAX_REPLY_BYTES),
        filter_result: Some(redact_secrets),
        ..TurnOptions::default()
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
    host: &TerminalHost<'_>,
    request: TerminalAssistRequestDto,
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    reply_streaming(generator, host, request, &mut |_| {}).await
}

/// What the terminal assistant needs from the application.
pub(crate) struct TerminalHost<'a> {
    /// The providers its working folder is opened through.
    pub(crate) providers: &'a ProviderRegistry,
    /// The per-tab sessions.
    pub(crate) sessions: &'a Sessions,
    /// The notes folder, when set up.
    pub(crate) notes: Option<&'a dyn NotesAccess>,
    /// The skill headers remembered between requests.
    pub(crate) skills: &'a Arc<SkillIndex>,
}

/// The folder an `ask` works in: the session's root when the working folder lies inside it
/// (task 0343), else the working folder.
fn tools_folder(request: &TerminalAssistRequestDto) -> &str {
    let working = request.working_directory.as_str();
    match request.root.as_deref() {
        Some(root) if !root.is_empty() && within(root, working) => root,
        _ => working,
    }
}

/// Whether location `inner` is `outer` or lies inside it.
fn within(outer: &str, inner: &str) -> bool {
    let outer = outer.trim_end_matches('/');
    inner
        .strip_prefix(outer)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// The files under a session's root, for `@` completion (task 0343).
pub(crate) async fn files(providers: &ProviderRegistry, root: &str) -> TerminalAssistFilesDto {
    commands::list_files(providers, root).await
}

/// The skills `/` can invoke in a folder (task 0343).
pub(crate) async fn skills(
    terminal: &TerminalHost<'_>,
    working_directory: &str,
) -> Vec<TerminalAssistSkillDto> {
    commands::list_skills(terminal, working_directory).await
}

/// Replaces a session's conversation with a summary (`/compact`, task 0343).
pub(crate) async fn compact(
    generator: &impl ChatGenerator,
    sessions: &Sessions,
    id: &str,
) -> Result<(), ChatFailureDto> {
    commands::compact(generator, sessions, id).await
}

/// Like [`reply`], and for an `ask` also reports each finished line of the answer, and each step,
/// to `on_event` while the model is still writing.
pub(crate) async fn reply_streaming(
    generator: &impl ChatGenerator,
    host: &TerminalHost<'_>,
    request: TerminalAssistRequestDto,
    on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    if request.kind == TerminalAssistKindDto::Ask {
        return ask(generator, host, request, on_event).await;
    }
    let (system_prompt, user_prompt) = prompts(&request)?;
    let raw = generate(
        generator,
        system_prompt,
        (user_prompt, Vec::new()),
        Vec::new(),
        &mut |_: &str| {},
    )
    .await?;
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
    (user_prompt, followups): (String, Vec<Message>),
    tools: Vec<ToolSpec>,
    on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<String, ChatFailureDto> {
    let cancellation = CancellationToken::new();
    generator
        .generate_for_role(
            LlmRole::Chat,
            LlmChatGeneration {
                followups,
                tools,
                system_prompt,
                user_prompt,
                images: Vec::new(),
                maximum_tokens: MAX_REPLY_TOKENS,
                context_window: None,
                temperature: None,
                reply: ReplyFormat::Text,
            },
            &cancellation,
            on_delta,
        )
        .await
        .map_err(chat_failure)
}

/// A session question: remembers the turn, lets the model use read-only tools, and stops at a
/// command that needs the person's approval.
async fn ask(
    generator: &impl ChatGenerator,
    terminal: &TerminalHost<'_>,
    request: TerminalAssistRequestDto,
    on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
    let (providers, sessions, notes) = (terminal.providers, terminal.sessions, terminal.notes);
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
    // The session's root, when the shell is still inside it, is the folder the tools may read.
    let folder = tools_folder(&request);
    let files = FolderTools::open(providers, folder);
    let (host, documents) = (files.host(), files.documents());
    // Calls are understood for every tool; the prompt describes `write_file` only when allowed.
    let registry = agent_tools::registry(true);
    let prompt_registry = agent_tools::registry(request.allow_file_writes);
    let mut steps = Vec::new();
    let mut question_text = String::new();

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
        question_text = question.to_owned();
        let files = attached_files(host, documents, &request.context.files).await;
        // The state goes in with the first question, and later only the parts that changed are
        // added: what was sent stays as it was, so a cached prompt prefix still matches and an
        // old command is never taken to have run in the new folder. Commands already sent in an
        // earlier turn are not sent again.
        let blocks = sessions.unsent_blocks(&id, &request.context.blocks);
        let state = sessions.state_update(&id, &state_sections(&request));
        let activity = render_activity(&blocks, &files);
        let skill = match request
            .skill
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            Some(name) => commands::invoked_skill(terminal, folder, name)
                .await
                .unwrap_or_else(|| format!("There is no skill called `{}`.", name.trim())),
            None => String::new(),
        };
        let context = [state, activity, skill]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        sessions.push_person(&id, question, &context);
    }

    let calling = tool_calling(generator).await;
    // A continuation after an approval has no new question: the notes that matched the last one
    // still apply.
    let skills_question = if question_text.is_empty() {
        sessions.last_question(&id)
    } else {
        question_text.clone()
    };
    let project = project_context(
        terminal.skills,
        providers,
        notes,
        folder,
        &skills_question,
        SkillScope::HostWide,
    )
    .await;
    let mut turn = TerminalTurn {
        generator,
        sessions,
        id: &id,
        system_prompt: ask_prompt(&prompt_registry, &project, &skills_question, calling),
        budget: session::conversation_budget(generator.context_window(LlmRole::Chat).await),
        calling,
        tools: prompt_registry.specs_for(calling),
    };
    let skill_documents = SkillFolderDocuments::new(providers);
    let side_calls = ModelSideCalls::new(generator, CancellationToken::new())
        .await
        .stopped_when(|| sessions.is_cancelled(&id));
    let ctx = ToolContext {
        notes,
        files: Some(&files),
        documents: Some(&skill_documents),
        side_model: Some(&side_calls),
        ..ToolContext::new(&project.skills)
    };
    let options = turn_options();
    loop {
        let end = run_turn(&mut turn, &registry, &ctx, &options, &mut |event| {
            on_event(match event {
                TurnEvent::Line(text) => TerminalAssistEventDto::Line { text },
                TurnEvent::Step(text) => {
                    steps.push(text.clone());
                    TerminalAssistEventDto::Step { text }
                }
                TurnEvent::Progress(text) => TerminalAssistEventDto::Progress { text },
            });
        })
        .await?;
        let interrupt = match end {
            TurnEnd::Answer(text) => return Ok(stopped(text, steps)),
            TurnEnd::EmptyReply => return Err(ChatFailureDto::Empty),
            TurnEnd::Cancelled => return Ok(stopped(String::new(), steps)),
            TurnEnd::StepLimit => {
                let text = "I stopped after many steps. Ask again to continue.".to_owned();
                return Ok(stopped(text, steps));
            }
            TurnEnd::Interrupted { call, interrupt } => {
                // What the person decides becomes this call's result.
                sessions.hold_call(&id, call);
                interrupt
            }
        };
        match interrupt {
            TerminalInterrupt::Note(proposal) => {
                sessions.push_tool(
                    &id,
                    "The note was shown to the person for review. Nothing is saved unless they \
                     save it.",
                );
                return Ok(TerminalAssistReplyDto {
                    note_proposal: Some(note_proposal_dto(proposal)),
                    ..stopped(String::new(), steps)
                });
            }
            TerminalInterrupt::Command(command) => {
                return Ok(TerminalAssistReplyDto {
                    run_command: Some(command.command),
                    ..stopped(String::new(), steps)
                });
            }
            TerminalInterrupt::WriteFile(pending) => {
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
            }
        }
    }
}

/// The terminal's side of a turn: the chat role, and the session that holds the conversation.
struct TerminalTurn<'a, G> {
    generator: &'a G,
    sessions: &'a Sessions,
    id: &'a str,
    system_prompt: String,
    calling: ToolCalling,
    tools: Vec<ToolSpec>,
    /// How many bytes of conversation the model's window allows.
    budget: usize,
}

#[async_trait]
impl<G: ChatGenerator> TurnHost for TerminalTurn<'_, G> {
    type Error = ChatFailureDto;

    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto> {
        let messages = self.sessions.messages_within(self.id, self.budget);
        generate(
            self.generator,
            self.system_prompt.clone(),
            generation_parts(messages, self.calling),
            self.tools.clone(),
            on_delta,
        )
        .await
    }

    fn push_assistant(&mut self, text: &str, calls: &[ToolCallRecord]) {
        self.sessions.push_assistant(self.id, text, calls);
    }

    fn push_result(&mut self, call: &ToolCallRecord, text: &str) {
        self.sessions.push_result(self.id, call, text);
    }

    fn push_note(&mut self, text: &str) {
        self.sessions.push_note(self.id, text);
    }

    fn is_cancelled(&self) -> bool {
        self.sessions.is_cancelled(self.id)
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use fm_agent::{NoteProposal, NoteToolCall, ToolOutcome};
    use fm_transport_dto::{TerminalContextDto, TerminalToolResultDto};

    use super::*;
    use crate::llm_profiles::LlmProfileError;

    fn skill_index() -> &'static Arc<SkillIndex> {
        static INDEX: std::sync::OnceLock<Arc<SkillIndex>> = std::sync::OnceLock::new();
        INDEX.get_or_init(|| Arc::new(SkillIndex::new()))
    }

    fn host<'a>(
        providers: &'a ProviderRegistry,
        sessions: &'a Sessions,
        notes: Option<&'a dyn NotesAccess>,
    ) -> TerminalHost<'a> {
        TerminalHost {
            providers,
            sessions,
            notes,
            skills: skill_index(),
        }
    }

    async fn reply_in(
        generator: &impl ChatGenerator,
        providers: &ProviderRegistry,
        sessions: &Sessions,
        notes: Option<&dyn NotesAccess>,
        request: TerminalAssistRequestDto,
    ) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
        super::reply(generator, &host(providers, sessions, notes), request).await
    }

    async fn reply_streaming_in(
        generator: &impl ChatGenerator,
        providers: &ProviderRegistry,
        sessions: &Sessions,
        notes: Option<&dyn NotesAccess>,
        request: TerminalAssistRequestDto,
        on_event: &mut (dyn FnMut(TerminalAssistEventDto) + Send),
    ) -> Result<TerminalAssistReplyDto, ChatFailureDto> {
        super::reply_streaming(
            generator,
            &host(providers, sessions, notes),
            request,
            on_event,
        )
        .await
    }

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
        /// Whether the endpoint takes tools natively.
        native: bool,
    }

    #[async_trait]
    impl ChatGenerator for FakeGenerator {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(false)
        }

        async fn native_tool_calling(&self, _role: LlmRole) -> bool {
            self.native
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
            native: false,
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
        async move { reply_in(fake, &ProviderRegistry::new(), &sessions, None, request).await }
    }

    fn command(request: &str) -> TerminalAssistRequestDto {
        TerminalAssistRequestDto {
            kind: TerminalAssistKindDto::Command,
            transcript: String::new(),
            request: Some(request.to_owned()),
            working_directory: "file:///projects/app".to_owned(),
            root: None,
            skill: None,
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
            root: None,
            skill: None,
            shell: None,
            session_id: None,
            context: TerminalContextDto::default(),
            tool_result: None,
            write_decision: None,
            allow_file_writes: false,
            workspace_id: None,
        }
    }

    #[test]
    fn the_intro_points_at_the_listing_and_prefers_tools_over_ls() {
        assert!(ASK_INTRO.contains("already names every file"));
        assert!(ASK_INTRO.contains("never suggest a command"));
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
            root: None,
            skill: None,
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
        reply_in(fake, &ProviderRegistry::new(), sessions, None, request).await
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
        assert!(seen[1].1.system_prompt.starts_with(ASK_INTRO));
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
    async fn a_summarised_file_stays_out_of_the_conversation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("paper.md"), "The full text of the paper.").unwrap();
        let fake = scripted(&[
            r#"TOOL {"tool":"summarize_file","path":"paper.md","focus":"decoys"}"#,
            "A short paper about decoys.",
            "Done: it is about decoys.",
        ]);
        let sessions = Sessions::default();
        let mut events = Vec::new();

        let reply = reply_streaming_in(
            &fake,
            &ProviderRegistry::new(),
            &sessions,
            None,
            ask_request("t1", "summarise paper.md", dir.path()),
            &mut |event| events.push(event),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, "Done: it is about decoys.");
        assert_eq!(reply.steps, vec!["Summarised paper.md".to_owned()]);
        // The person sees what the tool is doing while it runs, then its step.
        let progress = |text: &str| TerminalAssistEventDto::Progress { text: text.into() };
        assert_eq!(
            events[..3],
            [
                progress("Reading paper.md"),
                progress("Summarising paper.md"),
                TerminalAssistEventDto::Step {
                    text: "Summarised paper.md".into()
                },
            ]
        );
        let seen = fake.seen.lock().unwrap();
        // The side call: a fresh context with the document, and no tools.
        let side = &seen[1].1;
        assert!(side.user_prompt.contains("Focus: decoys"));
        assert!(side.user_prompt.contains("The full text of the paper."));
        assert!(side.tools.is_empty() && side.followups.is_empty());
        assert!(!side.system_prompt.contains("summarize_file"));
        // The conversation only gets the summary.
        let next = &seen[2].1.user_prompt;
        assert!(next.contains("Summary of paper.md"), "{next}");
        assert!(next.contains("A short paper about decoys."));
        assert!(!next.contains("The full text of the paper."));
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

    /// Talks to a real OpenAI-compatible endpoint with `curl`, native tools on.
    struct LiveGenerator {
        url: String,
        model: String,
    }

    #[async_trait]
    impl ChatGenerator for LiveGenerator {
        async fn accepts_images(&self, _role: LlmRole) -> Result<bool, LlmProfileError> {
            Ok(false)
        }

        async fn native_tool_calling(&self, _role: LlmRole) -> bool {
            true
        }

        async fn context_window(&self, _role: LlmRole) -> Option<u32> {
            Some(262_144)
        }

        async fn generate_for_role(
            &self,
            _role: LlmRole,
            generation: LlmChatGeneration,
            _cancellation: &CancellationToken,
            _on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
        ) -> Result<String, LlmProfileError> {
            let tools: Vec<serde_json::Value> = generation
                .tools
                .iter()
                .map(|tool| {
                    serde_json::json!({"type": "function", "function": {
                        "name": tool.name, "description": tool.description,
                        "parameters": tool.schema}})
                })
                .collect();
            let body = serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": generation.system_prompt},
                    {"role": "user", "content": generation.user_prompt}],
                "tools": tools, "temperature": 0.2, "max_tokens": 1024});
            let output = std::process::Command::new("curl")
                .args([
                    "-s",
                    "-m",
                    "300",
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                ])
                .arg(body.to_string())
                .arg(&self.url)
                .output()
                .expect("curl");
            let reply: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
            let message = &reply["choices"][0]["message"];
            let mut lines: Vec<String> = vec![message["content"].as_str().unwrap_or("").to_owned()];
            for call in message["tool_calls"].as_array().into_iter().flatten() {
                let mut args: serde_json::Value =
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("{}"))
                        .unwrap_or_default();
                args["tool"] = call["function"]["name"].clone();
                lines.push(format!("TOOL {args}"));
            }
            Ok(lines.join("\n"))
        }
    }

    /// Run by hand against a live model:
    /// `LIVE_LLM_URL=http://127.0.0.1:8000/v1/chat/completions LIVE_LLM_MODEL=... cargo nextest
    /// run -p fm-application live_model_summarises_a_folder --run-ignored all --no-capture`.
    #[tokio::test]
    #[ignore = "needs a live model"]
    async fn live_model_summarises_a_folder_into_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let count: usize = std::env::var("LIVE_FILES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4);
        for i in 1..=count {
            let body = format!("Topic {i} covers aspect {i} of the subject, with point item-{i}. ");
            std::fs::write(
                work.join(format!("doc{i}.md")),
                format!("# Doc {i}\n\n{}\n", body.repeat(60)),
            )
            .unwrap();
        }
        std::fs::copy(work.join("doc1.md"), work.join("doc1-copy.md")).unwrap();
        let fake = LiveGenerator {
            url: std::env::var("LIVE_LLM_URL").expect("LIVE_LLM_URL"),
            model: std::env::var("LIVE_LLM_MODEL").expect("LIVE_LLM_MODEL"),
        };
        let sessions = Sessions::default();
        let mut request = ask_request(
            "live",
            "Summarize all unique files in this folder into summaries.md",
            &work,
        );
        request.allow_file_writes = true;
        let mut reply = reply_in(&fake, &local_providers(), &sessions, None, request)
            .await
            .unwrap();
        println!("steps: {:?}\ntext: {}", reply.steps, reply.text);
        if reply.write_request.is_some() {
            let mut again = ask_request("live", "", &work);
            again.request = None;
            again.allow_file_writes = true;
            again.write_decision = Some(TerminalWriteDecisionDto::Session);
            reply = reply_in(&fake, &local_providers(), &sessions, None, again)
                .await
                .unwrap();
            println!("steps: {:?}\ntext: {}", reply.steps, reply.text);
        }
        let written = work.join("summaries/summaries.md");
        println!("written: {:?}", std::fs::read_to_string(written));
    }

    #[tokio::test]
    async fn a_new_folder_adds_its_state_and_keeps_the_old_one_in_place() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["first", "second"]);
        let sessions = Sessions::default();
        std::fs::write(first_dir.path().join("old.txt"), "x").unwrap();
        std::fs::write(second_dir.path().join("new.txt"), "x").unwrap();
        let one = ask_request("t1", "here?", first_dir.path());
        ask_in(&fake, &sessions, one).await.unwrap();
        let two = ask_request("t1", "now?", second_dir.path());
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

        let reply = reply_streaming_in(
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

        reply_streaming_in(
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
                TerminalAssistEventDto::Line { text }
                | TerminalAssistEventDto::Step { text }
                | TerminalAssistEventDto::Progress { text } => text,
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
        async fn run(&self, call: NoteToolCall) -> ToolOutcome<NoteProposal> {
            self.calls.lock().unwrap().push(call.clone());
            match call {
                NoteToolCall::Search { query, .. } => ToolOutcome::output(
                    format!("Searched notes for {query}"),
                    "1. ops/nginx.md: Replace nginx certificate",
                ),
                NoteToolCall::Read { path } => ToolOutcome::output(
                    format!("Read note {path}"),
                    "NOTE-BODY: sudo cp fullchain.pem /etc/nginx/",
                ),
                NoteToolCall::Propose { path, content, .. } => {
                    ToolOutcome::Interrupt(NoteProposal {
                        path,
                        title: "T".into(),
                        file_name: "t.md".into(),
                        tags: Vec::new(),
                        content,
                        redactions: 0,
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
        reply_in(fake, &ProviderRegistry::new(), sessions, notes, request).await
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
        reply_in(fake, &local_providers(), sessions, None, request)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn an_invoked_skill_goes_with_the_question() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join(".claude/skills/release");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Releases\n---\nBump, tag, push.",
        )
        .unwrap();
        let fake = scripted(&["Done."]);
        let sessions = Sessions::default();
        let request = TerminalAssistRequestDto {
            skill: Some("release".to_owned()),
            ..ask_request("t", "do it", dir.path())
        };
        ask_local(&fake, &sessions, request).await;
        let prompt = fake.seen.lock().unwrap()[0].1.user_prompt.clone();
        assert!(prompt.contains("invoked the skill `release`"), "{prompt}");
        assert!(prompt.contains("Bump, tag, push."), "{prompt}");

        let request = TerminalAssistRequestDto {
            skill: Some("missing".to_owned()),
            ..ask_request("t2", "do it", dir.path())
        };
        let fake = scripted(&["Done."]);
        ask_local(&fake, &sessions, request).await;
        let prompt = fake.seen.lock().unwrap()[0].1.user_prompt.clone();
        assert!(
            prompt.contains("There is no skill called `missing`."),
            "{prompt}"
        );
    }

    #[tokio::test]
    async fn the_session_root_is_the_tools_folder_while_the_shell_is_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.path().join("top.txt"), "from the root").unwrap();
        let root = url::Url::from_directory_path(dir.path())
            .unwrap()
            .to_string();
        let fake = scripted(&[r#"TOOL {"tool":"read_file","path":"top.txt"}"#, "ok"]);
        let sessions = Sessions::default();
        let request = TerminalAssistRequestDto {
            root: Some(root.clone()),
            ..ask_request("t", "read it", &sub)
        };
        ask_local(&fake, &sessions, request).await;
        assert!(sessions.render("t").contains("from the root"));

        // Outside the root, the working folder is used.
        let elsewhere = tempfile::tempdir().unwrap();
        let fake = scripted(&[r#"TOOL {"tool":"read_file","path":"top.txt"}"#, "ok"]);
        let request = TerminalAssistRequestDto {
            root: Some(root),
            ..ask_request("t2", "read it", elsewhere.path())
        };
        ask_local(&fake, &sessions, request).await;
        assert!(!sessions.render("t2").contains("from the root"));
    }

    #[test]
    fn a_location_is_within_its_root_only_at_a_folder_boundary() {
        assert!(within("file:///a/b/", "file:///a/b/c/"));
        assert!(within("file:///a/b", "file:///a/b"));
        assert!(!within("file:///a/b", "file:///a/bc"));
    }

    #[tokio::test]
    async fn compacting_replaces_the_conversation_with_the_summary() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["It is a Rust project.", "SUMMARY: a Rust project."]);
        let sessions = Sessions::default();
        ask_local(
            &fake,
            &sessions,
            ask_request("t", "what is this?", dir.path()),
        )
        .await;
        compact(&fake, &sessions, "t").await.unwrap();
        let rendered = sessions.render("t");
        assert!(rendered.contains("SUMMARY: a Rust project."), "{rendered}");
        assert!(!rendered.contains("what is this?"), "{rendered}");
        // Nothing to compact is not an error.
        compact(&fake, &sessions, "unknown").await.unwrap();
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
        let result = reply_in(
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
        let project = ProjectContext {
            skills: fm_agent::SkillCatalogue::default(),
            instructions: String::new(),
        };
        let off = agent_tools::registry(false);
        let on = agent_tools::registry(true);
        assert!(!ask_prompt(&off, &project, "", ToolCalling::Text).contains("write_file"));
        assert!(ask_prompt(&on, &project, "", ToolCalling::Text).contains("write_file"));
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

    // --- skills and notes as skills (task 0339) ---

    struct SkillNotes;

    #[async_trait]
    impl NotesAccess for SkillNotes {
        async fn run(&self, call: NoteToolCall) -> ToolOutcome<NoteProposal> {
            match call {
                NoteToolCall::Read { path } => ToolOutcome::output(
                    format!("Read note {path}"),
                    format!("Note {path}:\n1. certbot renew"),
                ),
                _ => ToolOutcome::output("Notes", "unused"),
            }
        }

        async fn skills(&self, _query: &str) -> Vec<fm_agent::NoteSkill> {
            vec![fm_agent::NoteSkill {
                name: "nginx-cert".to_owned(),
                description: "Use when replacing a certificate".to_owned(),
                path: "ops/nginx.md".to_owned(),
            }]
        }
    }

    fn project_skill(dir: &std::path::Path) {
        let skill = dir.join(".claude/skills/release");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: release\ndescription: Use when cutting a release\n---\nTag, then push.\n",
        )
        .unwrap();
        std::fs::write(skill.join("checklist.md"), "- changelog").unwrap();
    }

    #[tokio::test]
    async fn skills_and_matching_notes_are_in_the_prompt_and_load_on_demand() {
        let dir = tempfile::tempdir().unwrap();
        project_skill(dir.path());
        let fake = scripted(&[
            r#"TOOL {"tool":"read_skill","name":"release"}"#,
            r#"TOOL {"tool":"read_skill_file","name":"release","path":"checklist.md"}"#,
            r#"TOOL {"tool":"read_skill","name":"nginx-cert"}"#,
            "Done.",
        ]);
        let sessions = Sessions::default();
        let reply = reply_in(
            &fake,
            &ProviderRegistry::new(),
            &sessions,
            Some(&SkillNotes),
            ask_request("t", "cut a release and renew the certificate", dir.path()),
        )
        .await
        .unwrap();

        assert_eq!(reply.text, "Done.");
        assert_eq!(
            reply.steps,
            [
                "Loaded skill release",
                "Read checklist.md in skill release",
                "Loaded skill nginx-cert"
            ]
        );
        let prompt = &fake.seen.lock().unwrap()[0].1.system_prompt;
        assert!(prompt.contains("- read_skill: Loads a skill"));
        assert!(prompt.contains("- release: Use when cutting a release [project]"));
        assert!(prompt.contains("- nginx-cert: Use when replacing a certificate [note]"));
        let transcript = sessions.render("t");
        assert!(transcript.contains("Tag, then push."));
        assert!(transcript.contains("- changelog"));
        assert!(transcript.contains("1. certbot renew"));
    }

    #[tokio::test]
    async fn a_skill_script_is_never_run_and_bad_skill_arguments_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        project_skill(dir.path());
        let fake = scripted(&[
            r#"TOOL {"tool":"read_skill","name":"release","extra":1}"#,
            r#"TOOL {"tool":"read_skill_file","name":"release","path":"../../../etc/hosts"}"#,
            r#"TOOL {"tool":"read_skill"}"#,
            "Stopped.",
        ]);
        let sessions = Sessions::default();
        let reply = ask_in(&fake, &sessions, ask_request("t", "release", dir.path()))
            .await
            .unwrap();
        assert_eq!(reply.text, "Stopped.");
        assert!(reply.run_command.is_none());
        let transcript = sessions.render("t");
        assert!(transcript.contains("is not available."));
        assert!(transcript.contains("Error: invalid arguments for read_skill"));
    }

    #[tokio::test]
    async fn a_native_endpoint_gets_the_tools_as_a_list_not_in_the_prompt() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha").unwrap();
        let mut fake = scripted(&[
            r#"TOOL {"tool":"read_file","path":"a.txt"}"#,
            "It says alpha.",
        ]);
        fake.native = true;
        let sessions = Sessions::default();

        let reply = ask_local(
            &fake,
            &sessions,
            ask_request("t", "what is in a.txt?", dir.path()),
        )
        .await;

        assert_eq!(reply.text, "It says alpha.");
        assert_eq!(reply.steps, ["Read a.txt"]);
        let seen = fake.seen.lock().unwrap();
        let names: Vec<&str> = seen[0].1.tools.iter().map(|tool| tool.name).collect();
        assert!(names.contains(&"read_file") && names.contains(&"run_command"));
        assert!(!names.contains(&"write_file"));
        assert!(
            seen[0]
                .1
                .tools
                .iter()
                .all(|tool| tool.schema["type"] == "object")
        );
        let prompt = &seen[0].1.system_prompt;
        assert!(prompt.starts_with(ASK_INTRO));
        assert!(!prompt.contains("TOOL {json}"));
        assert!(!prompt.contains("- read_file"));
    }

    #[tokio::test]
    async fn a_text_protocol_endpoint_gets_no_tools_list() {
        let dir = tempfile::tempdir().unwrap();
        let fake = scripted(&["Hello."]);
        let sessions = Sessions::default();
        ask_local(&fake, &sessions, ask_request("t", "hi", dir.path())).await;
        let seen = fake.seen.lock().unwrap();
        assert!(seen[0].1.tools.is_empty());
        assert!(seen[0].1.system_prompt.contains("TOOL {json}"));
        assert!(seen[0].1.system_prompt.contains("- read_file"));
    }
}
