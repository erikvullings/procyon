//! The chat with tools (task 0339): the same agent loop as the terminal assistant, with the
//! read-only registry. The model may list, read and search the folder the person chose, look
//! through their notes and skills, and propose a note for review. It cannot write, run commands or
//! leave the folder. Whatever it reads goes to the chat endpoint under the consent that already
//! covers the conversation: a cloud endpoint without consent fails before anything is sent.

use async_trait::async_trait;
use fm_agent::{
    NotesAccess, ToolContext, TurnEnd, TurnEvent, TurnHost, read_only_registry, run_turn,
};
use fm_transport_dto::{ChatFailureDto, ChatReplyEventDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

use crate::chat::ChatGenerator;
use crate::llm_profiles::{
    LlmChatGeneration, LlmImageAttachment, LlmProfileError, LlmRole, LlmToolSpec,
};
use crate::terminal_assist::{FolderTools, SkillFolderDocuments, skills_for};

/// Most transcript bytes kept while tools run; the oldest part drops out first.
const MAX_TRANSCRIPT_BYTES: usize = 192 * 1024;
/// Longest tool result added to the transcript, in bytes.
const MAX_TOOL_BYTES: usize = 16 * 1024;
/// Longest step line kept with a reply, in bytes.
const MAX_STEP_BYTES: usize = 200;
/// No limit of its own on generated tokens: the profile's answer limit (set from the model's
/// own limits) is the only bound, so a long answer or a file's content is not cut short.
const MAX_REPLY_TOKENS: u32 = u32::MAX;

const TOOLS_INTRO: &str = "\nYou can look things up in the folder the person chose, in their \
     notes and in the skills listed below. Paths are relative to that folder and cannot leave it. \
     Look things up instead of guessing, and never say you will look something up without making \
     the call in the same reply. You still cannot change files or run commands.";
const TEXT_PROTOCOL: &str = "\nTo use a tool, reply with exactly one line and nothing else: \
     `TOOL {json}`. Then wait for the result.";

/// What the chat needs from the application to run tools.
pub(crate) struct ChatTooling<'a> {
    /// The providers the folder is opened through.
    pub(crate) providers: &'a ProviderRegistry,
    /// The notes folder, when one is set.
    pub(crate) notes: Option<&'a dyn NotesAccess>,
    /// Whether the skills of the host's home folder and the folders above the working folder may
    /// be used. False for browser clients of a server.
    pub(crate) host_wide_skills: bool,
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
    let registry = read_only_registry();
    let folder = FolderTools::open(tooling.providers, input.folder);
    let files = folder.files();
    let catalogue = skills_for(
        tooling.notes,
        input.folder,
        input.question,
        tooling.host_wide_skills,
    )
    .await;
    let native = generator.native_tool_calling(LlmRole::Chat).await;
    let mut system_prompt = format!("{}{TOOLS_INTRO}", input.system_prompt);
    if !native {
        system_prompt.push_str(TEXT_PROTOCOL);
        system_prompt.push('\n');
        system_prompt.push_str(&registry.prompt_section());
    }
    let skills = catalogue.prompt_section(input.question);
    if !skills.is_empty() {
        system_prompt.push('\n');
        system_prompt.push_str(&skills);
    }
    let tools = if native {
        registry
            .specs()
            .into_iter()
            .map(|spec| LlmToolSpec {
                name: spec.name.to_owned(),
                description: spec.description.to_owned(),
                schema: spec.schema,
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut turn = ChatTurn {
        generator,
        system_prompt,
        transcript: input.transcript,
        tools,
        images: input.images,
        cancellation: input.cancellation,
    };
    let skill_documents = SkillFolderDocuments::new(tooling.providers);
    let ctx = ToolContext {
        skills: &catalogue,
        notes: tooling.notes,
        files: Some(&files),
        documents: Some(&skill_documents),
    };
    let mut steps = Vec::new();
    let end = run_turn(&mut turn, &registry, &ctx, &mut steps, &mut |event| {
        on_event(match event {
            TurnEvent::Line(text) => ChatReplyEventDto::Delta {
                text: format!("{text}\n"),
            },
            TurnEvent::Step(text) => ChatReplyEventDto::Step { text },
        });
    })
    .await?;
    // Kept with the reply, short, so the lookups stay visible when the conversation is reopened.
    let steps: Vec<String> = steps
        .into_iter()
        .map(|step| fm_agent::truncate_to_bytes(&step, MAX_STEP_BYTES).to_owned())
        .collect();
    match end {
        TurnEnd::Answer(text) => Ok((AgentEnd::Answer(text), steps)),
        TurnEnd::Cancelled => Err(ChatFailureDto::Cancelled),
        TurnEnd::StepLimit => Ok((
            AgentEnd::Answer("I stopped after many steps. Ask again to continue.".to_owned()),
            steps,
        )),
        TurnEnd::Proposal(proposal) => Ok((AgentEnd::Proposal(proposal), steps)),
        // The read-only registry has no tool that needs approval.
        TurnEnd::NeedsApproval(_) => Err(ChatFailureDto::Unavailable),
    }
}

/// The chat's side of a turn: the chat role and the transcript so far.
struct ChatTurn<'a, G> {
    generator: &'a G,
    system_prompt: String,
    transcript: String,
    tools: Vec<LlmToolSpec>,
    images: Vec<LlmImageAttachment>,
    cancellation: &'a CancellationToken,
}

impl<G> ChatTurn<'_, G> {
    fn push(&mut self, text: &str) {
        self.transcript.push_str(text);
        if self.transcript.len() > MAX_TRANSCRIPT_BYTES {
            let mut cut = self.transcript.len() - MAX_TRANSCRIPT_BYTES;
            while !self.transcript.is_char_boundary(cut) {
                cut += 1;
            }
            self.transcript.drain(..cut);
        }
    }
}

#[async_trait]
impl<G: ChatGenerator> TurnHost for ChatTurn<'_, G> {
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
                    user_prompt: self.transcript.clone(),
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
            .map_err(|error| match error {
                LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
                LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
                LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
                _ => ChatFailureDto::Unavailable,
            })
    }

    fn push_assistant(&mut self, text: &str) {
        self.push(&format!("Assistant: {text}\n\n"));
    }

    fn push_tool(&mut self, text: &str) {
        let text = fm_agent::truncate_to_bytes(text, MAX_TOOL_BYTES);
        self.push(&format!("Tool result:\n{text}\n\n"));
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}
