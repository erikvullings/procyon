//! The chat with tools (task 0339): the same agent loop as the terminal assistant, with the
//! read-only registry. The model may list, read and search the folder the person chose, look
//! through their notes and skills, and propose a note for review. It cannot write, run commands or
//! leave the folder. Whatever it reads goes to the chat endpoint under the consent that already
//! covers the conversation: a cloud endpoint without consent fails before anything is sent.

use std::sync::Arc;

use async_trait::async_trait;
use fm_agent::{
    Message, NoteProposal, NotesAccess, SkillIndex, ToolCallRecord, ToolCalling, ToolContext,
    ToolSpec, TurnEnd, TurnEvent, TurnHost, TurnOptions, compose_system_prompt, fit_messages,
    read_only_registry, redact_secrets, run_turn, truncate_to_bytes,
};
use fm_transport_dto::{ChatFailureDto, ChatReplyEventDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use crate::agent_host::{
    FolderTools, ModelSideCalls, NamedHost, READ_FILE_DESCRIPTION, SkillFolderDocuments,
    SkillScope, WebReader, chat_failure, generation_parts, note_proposal_dto, project_context,
    tool_calling,
};
use crate::chat::ChatGenerator;
use crate::llm_profiles::{
    LlmChatGeneration, LlmImageAttachment, LlmRole, ReplyFormat, ThinkingRequest,
};

/// Most bytes of tool exchanges kept while tools run; the oldest exchanges drop out first. The
/// conversation itself (with the person's question) is bounded by the chat and always kept.
const MAX_EXCHANGE_BYTES: usize = 192 * 1024;
/// Longest tool result added to the transcript, in bytes.
const MAX_TOOL_BYTES: usize = 16 * 1024;
/// Longest step line kept with a reply, in bytes.
const MAX_STEP_BYTES: usize = 200;
/// Most bytes of tool exchanges saved with a reply (task 0359); the oldest drop out first.
const MAX_SAVED_EXCHANGE_BYTES: usize = 24 * 1024;

const TOOLS_INTRO: &str = "\nYou can look things up in the folder the person chose, in their \
     notes and in the skills listed below. Paths are relative to that folder and cannot leave it. \
     Look things up instead of guessing, and never say you will look something up without making \
     the call in the same reply. You still cannot change files or run commands.";

/// What the chat needs from the application to run tools.
pub(crate) struct ChatTooling<'a> {
    /// The providers the folder is opened through.
    pub(crate) providers: &'a ProviderRegistry,
    /// The notes folder, when one is set.
    pub(crate) notes: Option<&'a dyn NotesAccess>,
    /// Which skill folders may be read.
    pub(crate) skill_scope: SkillScope,
    /// The skill headers remembered between requests.
    pub(crate) skills: &'a Arc<SkillIndex>,
}

/// How a chat turn with tools ended.
pub(crate) enum AgentEnd {
    /// The model answered.
    Answer(String),
    /// The model drafted a note for the person to review.
    Proposal(NoteProposalDto),
}

/// A finished chat turn with tools.
pub(crate) struct AgentReply {
    pub(crate) end: AgentEnd,
    /// The step lines shown while it ran.
    pub(crate) steps: Vec<String>,
    /// The model's calls and their (redacted, bounded) results, without the final answer, to be
    /// saved with the reply so a later question still has them (task 0359).
    pub(crate) exchanges: Vec<Message>,
}

/// Everything one turn with tools needs besides the model.
pub(crate) struct AgentRun<'a> {
    pub(crate) system_prompt: &'a str,
    pub(crate) folder: &'a str,
    pub(crate) question: &'a str,
    pub(crate) transcript: String,
    pub(crate) images: Vec<LlmImageAttachment>,
    /// Whether `web_fetch` may read public web pages (task 0363).
    pub(crate) allow_web: bool,
    /// Local hosts the person named in messages they typed, which `web_fetch` may read (0365).
    pub(crate) named_hosts: Vec<NamedHost>,
    /// The conversation's thinking effort (task 0361).
    pub(crate) thinking: ThinkingRequest,
    pub(crate) cancellation: &'a CancellationToken,
}

/// Runs one chat turn with the read-only tools.
pub(crate) async fn run(
    generator: &impl ChatGenerator,
    tooling: &ChatTooling<'_>,
    input: AgentRun<'_>,
    on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
) -> Result<AgentReply, ChatFailureDto> {
    let mut registry = read_only_registry::<NoteProposal>();
    if !input.allow_web {
        // Off: the model is not told about a tool it may not use.
        registry.remove("web_fetch");
    }
    let web = WebReader::new(tooling.providers).allowing(input.named_hosts);
    registry.describe("read_file", READ_FILE_DESCRIPTION);
    let files = FolderTools::open(tooling.providers, input.folder);
    let project = project_context(
        tooling.skills,
        tooling.providers,
        tooling.notes,
        input.folder,
        input.question,
        tooling.skill_scope,
    )
    .await;
    let catalogue = project.skills;
    let (calling, profile) = tool_calling(generator).await;
    let mut base = format!("{}{TOOLS_INTRO}", input.system_prompt);
    if !project.instructions.is_empty() {
        base.push_str("\n\n");
        base.push_str(&project.instructions);
    }
    profile.append_prompt_notes(&mut base);
    let mut turn = ChatTurn {
        generator,
        system_prompt: compose_system_prompt(&base, &registry, calling, &catalogue, input.question),
        conversation: input.transcript,
        exchanges: Vec::new(),
        calling,
        tools: registry.specs_for(calling),
        images: input.images,
        thinking: input.thinking,
        cancellation: input.cancellation,
    };
    let skill_documents = SkillFolderDocuments::new(tooling.providers);
    let side_calls = ModelSideCalls::new(generator, input.cancellation.clone()).await;
    let ctx = ToolContext {
        notes: tooling.notes,
        files: Some(&files),
        documents: Some(&skill_documents),
        side_model: Some(&side_calls),
        web: input.allow_web.then_some(&web as &dyn fm_agent::WebAccess),
        ..ToolContext::new(&catalogue)
    };
    let options = TurnOptions {
        profile,
        // The chat bounds the saved answer itself.
        max_answer_bytes: None,
        filter_result: Some(redact_secrets),
        ..TurnOptions::default()
    };
    let mut steps = Vec::new();
    let end = run_turn(&mut turn, &registry, &ctx, &options, &mut |event| {
        on_event(match event {
            TurnEvent::Line(text) => ChatReplyEventDto::Delta {
                text: format!("{text}\n"),
            },
            TurnEvent::Step(text) | TurnEvent::LoopStopped(text) => {
                // Kept with the reply, short, so the lookups stay visible when the conversation
                // is reopened.
                steps.push(truncate_to_bytes(&text, MAX_STEP_BYTES).to_owned());
                ChatReplyEventDto::Step { text }
            }
            TurnEvent::Progress(text) => ChatReplyEventDto::Progress { text },
            // Chat sends no messages during a reply, so nothing is ever steered here.
            TurnEvent::Steered(text) => ChatReplyEventDto::Step { text },
        });
    })
    .await?;
    let end = match end {
        TurnEnd::Answer(text) | TurnEnd::LoopStopped(text) => AgentEnd::Answer(text),
        TurnEnd::EmptyReply => return Err(ChatFailureDto::Empty),
        TurnEnd::Cancelled => return Err(ChatFailureDto::Cancelled),
        TurnEnd::StepLimit => {
            AgentEnd::Answer("I stopped after many steps. Ask again to continue.".to_owned())
        }
        TurnEnd::Interrupted { interrupt, .. } => AgentEnd::Proposal(note_proposal_dto(interrupt)),
    };
    Ok(AgentReply {
        end,
        steps,
        exchanges: saved_exchanges(turn.exchanges),
    })
}

/// The exchanges worth saving with a reply: the calls and their results, without the final answer
/// (saved as the reply's text), the newest within [`MAX_SAVED_EXCHANGE_BYTES`].
fn saved_exchanges(mut exchanges: Vec<Message>) -> Vec<Message> {
    if matches!(exchanges.last(), Some(Message::Assistant { calls, .. }) if calls.is_empty()) {
        exchanges.pop();
    }
    if !exchanges
        .iter()
        .any(|message| matches!(message, Message::Assistant { calls, .. } if !calls.is_empty()))
    {
        return Vec::new();
    }
    fit_messages(&exchanges, MAX_SAVED_EXCHANGE_BYTES)
}

/// The chat's side of a turn: the chat role, the conversation, and this answer's tool exchanges.
struct ChatTurn<'a, G> {
    generator: &'a G,
    system_prompt: String,
    /// The conversation up to the person's question; always sent whole.
    conversation: String,
    /// This answer's replies, results and notes, oldest first.
    exchanges: Vec<Message>,
    calling: ToolCalling,
    tools: Vec<ToolSpec>,
    images: Vec<LlmImageAttachment>,
    thinking: ThinkingRequest,
    cancellation: &'a CancellationToken,
}

impl<G> ChatTurn<'_, G> {
    /// The conversation and as many of the newest exchanges as fit: whole replies with their
    /// results, the oldest left out first with a note.
    fn messages(&self) -> Vec<Message> {
        let mut all = Vec::with_capacity(self.exchanges.len() + 1);
        all.push(Message::User(self.conversation.clone()));
        all.extend(self.exchanges.iter().cloned());
        fit_messages(&all, self.conversation.len() + MAX_EXCHANGE_BYTES)
    }
}

#[async_trait]
impl<G: ChatGenerator> TurnHost for ChatTurn<'_, G> {
    type Error = ChatFailureDto;

    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto> {
        let (user_prompt, followups) = generation_parts(self.messages(), self.calling);
        self.generator
            .generate_for_role(
                LlmRole::Chat,
                LlmChatGeneration {
                    followups,
                    tools: self.tools.clone(),
                    system_prompt: self.system_prompt.clone(),
                    user_prompt,
                    // Each provider call sees the current message, including its images.
                    images: self.images.clone(),
                    maximum_tokens: None,
                    context_window: None,
                    temperature: None,
                    reply: ReplyFormat::Text,
                    thinking: self.thinking,
                },
                self.cancellation,
                on_delta,
            )
            .await
            .map_err(chat_failure)
    }

    fn push_assistant(&mut self, text: &str, calls: &[ToolCallRecord]) {
        self.exchanges.push(Message::Assistant {
            text: text.to_owned(),
            calls: calls.to_vec(),
        });
    }

    fn push_result(&mut self, call: &ToolCallRecord, text: &str) {
        let text = truncate_to_bytes(text, MAX_TOOL_BYTES);
        self.exchanges.push(Message::result(call, text));
    }

    fn push_note(&mut self, text: &str) {
        self.exchanges.push(Message::Note(text.to_owned()));
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use fm_agent::{OMITTED_NOTE, render_text};
    use serde_json::json;

    use super::*;

    struct NoModel;

    fn turn() -> ChatTurn<'static, NoModel> {
        static CANCEL: std::sync::OnceLock<CancellationToken> = std::sync::OnceLock::new();
        ChatTurn {
            generator: &NoModel,
            system_prompt: String::new(),
            conversation: "Person: what is in the big file?\n".to_owned(),
            exchanges: Vec::new(),
            calling: ToolCalling::Text,
            tools: Vec::new(),
            images: Vec::new(),
            thinking: ThinkingRequest::Background,
            cancellation: CANCEL.get_or_init(CancellationToken::new),
        }
    }

    fn call(n: usize) -> ToolCallRecord {
        ToolCallRecord {
            id: format!("c{n}"),
            name: "read_file".into(),
            arguments: json!({"path": n.to_string()}),
        }
    }

    fn reply(turn: &mut ChatTurn<'_, NoModel>, text: &str, calls: &[ToolCallRecord]) {
        turn.exchanges.push(Message::Assistant {
            text: text.to_owned(),
            calls: calls.to_vec(),
        });
    }

    fn result(turn: &mut ChatTurn<'_, NoModel>, call: &ToolCallRecord, text: &str) {
        turn.exchanges.push(Message::result(call, text));
    }

    #[test]
    fn the_question_stays_when_old_tool_results_drop_out() {
        let mut turn = turn();
        for n in 0..40 {
            reply(&mut turn, &format!("TOOL read {n}"), &[call(n)]);
            result(
                &mut turn,
                &call(n),
                &format!("{n}{}", "x".repeat(10 * 1024)),
            );
        }
        let transcript = render_text(&turn.messages());
        assert!(transcript.starts_with("Person: what is in the big file?\n"));
        assert!(transcript.contains(OMITTED_NOTE));
        assert!(transcript.contains("Tool result:\n39"));
        assert!(!transcript.contains("Tool result:\n0x"));
        assert!(transcript.len() <= turn.conversation.len() + MAX_EXCHANGE_BYTES + 1024);
    }

    #[test]
    fn the_newest_result_is_kept_even_when_it_alone_is_large() {
        let mut turn = turn();
        reply(&mut turn, "small", &[call(0)]);
        result(&mut turn, &call(0), "small");
        reply(&mut turn, "big", &[call(1)]);
        turn.exchanges.push(Message::result(
            &call(1),
            "y".repeat(MAX_EXCHANGE_BYTES + 10),
        ));
        let messages = turn.messages();
        assert!(matches!(&messages[0], Message::User(text) if text.starts_with("Person:")));
        assert!(
            matches!(messages.last(), Some(Message::ToolResult { call_id, .. }) if call_id == "c1")
        );
    }

    #[test]
    fn natively_the_question_is_the_prompt_and_the_exchanges_follow_as_messages() {
        let mut turn = turn();
        turn.calling = ToolCalling::Native;
        reply(&mut turn, "Let me look.", &[call(0)]);
        result(&mut turn, &call(0), "content");
        let (prompt, followups) = generation_parts(turn.messages(), turn.calling);
        assert_eq!(prompt, turn.conversation);
        assert_eq!(followups.len(), 2);
        assert!(matches!(&followups[1], Message::ToolResult { call_id, .. } if call_id == "c0"));
    }
}
