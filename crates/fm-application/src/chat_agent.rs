//! The chat with tools (task 0339): the same agent loop as the terminal assistant, with the
//! read-only registry. The model may list, read and search the folder the person chose, look
//! through their notes and skills, and propose a note for review. It cannot write, run commands or
//! leave the folder. Whatever it reads goes to the chat endpoint under the consent that already
//! covers the conversation: a cloud endpoint without consent fails before anything is sent.

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use fm_agent::{
    NoteProposal, NotesAccess, SkillIndex, ToolContext, ToolSpec, TurnEnd, TurnEvent, TurnHost,
    TurnOptions, compose_system_prompt, read_only_registry, redact_secrets, run_turn,
    truncate_to_bytes,
};
use fm_transport_dto::{ChatFailureDto, ChatReplyEventDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use crate::agent_host::{
    FolderTools, READ_FILE_DESCRIPTION, SkillFolderDocuments, SkillScope, chat_failure,
    note_proposal_dto, skills_for, tool_calling,
};
use crate::chat::ChatGenerator;
use crate::llm_profiles::{LlmChatGeneration, LlmImageAttachment, LlmRole};

/// Most bytes of tool exchanges kept while tools run; the oldest exchanges drop out first. The
/// conversation itself (with the person's question) is bounded by the chat and always kept.
const MAX_EXCHANGE_BYTES: usize = 192 * 1024;
/// Longest tool result added to the transcript, in bytes.
const MAX_TOOL_BYTES: usize = 16 * 1024;
/// Longest step line kept with a reply, in bytes.
const MAX_STEP_BYTES: usize = 200;
/// No limit of its own on generated tokens: the profile's answer limit (set from the model's
/// own limits) is the only bound, so a long answer or a file's content is not cut short.
const MAX_REPLY_TOKENS: u32 = u32::MAX;
/// Put in place of tool exchanges that no longer fit.
const OMITTED: &str = "[Earlier tool results of this answer were left out to save space.]\n\n";

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

/// Everything one turn with tools needs besides the model.
pub(crate) struct AgentRun<'a> {
    pub(crate) system_prompt: &'a str,
    pub(crate) folder: &'a str,
    pub(crate) question: &'a str,
    pub(crate) transcript: String,
    pub(crate) images: Vec<LlmImageAttachment>,
    pub(crate) cancellation: &'a CancellationToken,
}

/// Runs one chat turn with the read-only tools.
pub(crate) async fn run(
    generator: &impl ChatGenerator,
    tooling: &ChatTooling<'_>,
    input: AgentRun<'_>,
    on_event: &mut (dyn FnMut(ChatReplyEventDto) + Send),
) -> Result<(AgentEnd, Vec<String>), ChatFailureDto> {
    let mut registry = read_only_registry::<NoteProposal>();
    registry.describe("read_file", READ_FILE_DESCRIPTION);
    let files = FolderTools::open(tooling.providers, input.folder);
    let catalogue = skills_for(
        tooling.skills,
        tooling.notes,
        input.folder,
        input.question,
        tooling.skill_scope,
    )
    .await;
    let calling = tool_calling(generator).await;
    let base = format!("{}{TOOLS_INTRO}", input.system_prompt);
    let mut turn = ChatTurn {
        generator,
        system_prompt: compose_system_prompt(&base, &registry, calling, &catalogue, input.question),
        conversation: input.transcript,
        exchanges: VecDeque::new(),
        exchange_bytes: 0,
        omitted: false,
        tools: registry.specs_for(calling),
        images: input.images,
        cancellation: input.cancellation,
    };
    let skill_documents = SkillFolderDocuments::new(tooling.providers);
    let ctx = ToolContext {
        notes: tooling.notes,
        files: Some(&files),
        documents: Some(&skill_documents),
        ..ToolContext::new(&catalogue)
    };
    let options = TurnOptions {
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
            TurnEvent::Step(text) => {
                // Kept with the reply, short, so the lookups stay visible when the conversation
                // is reopened.
                steps.push(truncate_to_bytes(&text, MAX_STEP_BYTES).to_owned());
                ChatReplyEventDto::Step { text }
            }
        });
    })
    .await?;
    match end {
        TurnEnd::Answer(text) => Ok((AgentEnd::Answer(text), steps)),
        TurnEnd::EmptyReply => Err(ChatFailureDto::Empty),
        TurnEnd::Cancelled => Err(ChatFailureDto::Cancelled),
        TurnEnd::StepLimit => Ok((
            AgentEnd::Answer("I stopped after many steps. Ask again to continue.".to_owned()),
            steps,
        )),
        TurnEnd::Interrupted(proposal) => {
            Ok((AgentEnd::Proposal(note_proposal_dto(proposal)), steps))
        }
    }
}

/// The chat's side of a turn: the chat role, the conversation, and this answer's tool exchanges.
struct ChatTurn<'a, G> {
    generator: &'a G,
    system_prompt: String,
    /// The conversation up to the person's question; always sent whole.
    conversation: String,
    /// This answer's replies and tool results, oldest first.
    exchanges: VecDeque<String>,
    exchange_bytes: usize,
    /// Whether old exchanges were dropped.
    omitted: bool,
    tools: Vec<ToolSpec>,
    images: Vec<LlmImageAttachment>,
    cancellation: &'a CancellationToken,
}

impl<G> ChatTurn<'_, G> {
    fn push(&mut self, text: String) {
        self.exchange_bytes += text.len();
        self.exchanges.push_back(text);
        // The newest exchange always stays, so the model sees the result it is waiting for.
        while self.exchange_bytes > MAX_EXCHANGE_BYTES && self.exchanges.len() > 1 {
            if let Some(dropped) = self.exchanges.pop_front() {
                self.exchange_bytes -= dropped.len();
                self.omitted = true;
            }
        }
    }

    fn transcript(&self) -> String {
        let mut text = String::with_capacity(
            self.conversation.len() + self.exchange_bytes + OMITTED.len() + 2,
        );
        text.push_str(&self.conversation);
        if !self.exchanges.is_empty() && !text.ends_with("\n\n") {
            text.push_str(if text.ends_with('\n') { "\n" } else { "\n\n" });
        }
        if self.omitted {
            text.push_str(OMITTED);
        }
        for exchange in &self.exchanges {
            text.push_str(exchange);
        }
        text
    }
}

#[async_trait]
impl<G: ChatGenerator> TurnHost for ChatTurn<'_, G> {
    type Error = ChatFailureDto;

    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto> {
        self.generator
            .generate_for_role(
                LlmRole::Chat,
                LlmChatGeneration {
                    tools: self.tools.clone(),
                    system_prompt: self.system_prompt.clone(),
                    user_prompt: self.transcript(),
                    // Images belong to the person's message, so they are sent once.
                    images: std::mem::take(&mut self.images),
                    maximum_tokens: MAX_REPLY_TOKENS,
                    context_window: None,
                    temperature: None,
                },
                self.cancellation,
                on_delta,
            )
            .await
            .map_err(chat_failure)
    }

    fn push_assistant(&mut self, text: &str) {
        self.push(format!("Assistant: {text}\n\n"));
    }

    fn push_tool(&mut self, text: &str) {
        let text = truncate_to_bytes(text, MAX_TOOL_BYTES);
        self.push(format!("Tool result:\n{text}\n\n"));
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoModel;

    fn turn() -> ChatTurn<'static, NoModel> {
        static CANCEL: std::sync::OnceLock<CancellationToken> = std::sync::OnceLock::new();
        ChatTurn {
            generator: &NoModel,
            system_prompt: String::new(),
            conversation: "Person: what is in the big file?\n".to_owned(),
            exchanges: VecDeque::new(),
            exchange_bytes: 0,
            omitted: false,
            tools: Vec::new(),
            images: Vec::new(),
            cancellation: CANCEL.get_or_init(CancellationToken::new),
        }
    }

    #[test]
    fn the_question_stays_when_old_tool_results_drop_out() {
        let mut turn = turn();
        for n in 0..40 {
            turn.push(format!("Tool result:\n{n}{}\n\n", "x".repeat(10 * 1024)));
        }
        let transcript = turn.transcript();
        assert!(transcript.starts_with("Person: what is in the big file?\n"));
        assert!(transcript.contains(OMITTED));
        assert!(transcript.contains("Tool result:\n39"));
        assert!(!transcript.contains("Tool result:\n0x"));
        assert!(turn.exchange_bytes <= MAX_EXCHANGE_BYTES);
    }

    #[test]
    fn the_newest_result_is_kept_even_when_it_alone_is_large() {
        let mut turn = turn();
        turn.push("small\n\n".to_owned());
        turn.push("y".repeat(MAX_EXCHANGE_BYTES + 10));
        assert_eq!(turn.exchanges.len(), 1);
        assert!(turn.transcript().starts_with("Person:"));
    }
}
