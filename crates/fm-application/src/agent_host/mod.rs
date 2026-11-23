//! Procyon's side of the agent harness (task 0339), shared by the terminal assistant and the chat:
//! file tools over one working folder (local, or remote through its provider, with documents read
//! as text), the skills of a request, and the mapping of harness results to wire DTOs. `fm-agent`
//! knows nothing of providers, settings or DTOs; this module is where they meet.

mod documents;
mod remote;
mod sandbox;
mod skill_store;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use fm_agent::{
    FileTools, InstructionSource, MAX_INSTRUCTION_BYTES, Message, NoteProposal, NotesAccess,
    SideModel, SideModelError, SkillCatalogue, SkillIndex, SkillRoots, StoreHandle, ToolCalling,
    ToolOutput, WholeText, instruction_candidates, instructions_section, read_instructions,
    render_text,
};
use fm_transport_dto::{ChatFailureDto, NoteDraftDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;
use tokio_util::sync::CancellationToken;

pub(crate) use self::documents::{Documents, SkillFolderDocuments, is_document};
use self::sandbox::continuation;
pub(crate) use self::sandbox::{FileCall, ToolHost};
use self::skill_store::ProviderStore;
use crate::chat::ChatGenerator;
use crate::llm_profiles::{LlmChatGeneration, LlmProfileError, LlmRole, ReplyFormat};

/// The local path of a `file:` location, or `None` for any other location.
pub(crate) fn path_from_location(uri: &str) -> Option<PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

/// The read-only file tools over one working folder.
pub(crate) struct FolderTools {
    host: Option<ToolHost>,
    documents: Documents,
}

impl FolderTools {
    /// Opens the folder named by `location`; the tools answer with an error when it cannot be used.
    pub(crate) fn open(providers: &ProviderRegistry, location: &str) -> Self {
        Self {
            host: ToolHost::open(providers, location),
            documents: Documents::new(providers),
        }
    }

    /// The folder, when it could be opened.
    pub(crate) fn host(&self) -> Option<&ToolHost> {
        self.host.as_ref()
    }

    /// The document reader of the folder.
    pub(crate) fn documents(&self) -> &Documents {
        &self.documents
    }

    async fn run(&self, call: FileCall) -> ToolOutput {
        match &self.host {
            Some(host) => match self.documents.run(host, &call).await {
                Some(output) => output,
                None => host.run(call).await,
            },
            None => ToolOutput::error(
                "Looked up a file",
                "file tools are not available for this location.",
            ),
        }
    }
}

#[async_trait]
impl FileTools for FolderTools {
    async fn list_dir(&self, path: &str) -> ToolOutput {
        self.run(FileCall::ListDir {
            path: path.to_owned(),
        })
        .await
    }

    async fn read_file(&self, path: &str, offset: usize) -> ToolOutput {
        self.run(FileCall::ReadFile {
            path: path.to_owned(),
            offset: Some(offset),
        })
        .await
    }

    async fn grep(&self, pattern: &str, path: Option<&str>) -> ToolOutput {
        self.run(FileCall::Grep {
            pattern: pattern.to_owned(),
            path: path.map(str::to_owned),
        })
        .await
    }

    async fn read_whole(&self, path: &str, max: usize) -> Result<WholeText, String> {
        // `read_file` page by page, each continued at the offset its cut-off note names.
        let mut text = String::new();
        let mut offset = 0;
        loop {
            let page = self.read_file(path, offset).await.text;
            if let Some(message) = page.strip_prefix("Error: ") {
                return Err(message.to_owned());
            }
            let (body, next) = continuation(&page);
            text.push_str(body);
            match next {
                Some(next) if next > offset && text.len() < max => offset = next,
                Some(_) => {
                    return Ok(WholeText {
                        text,
                        truncated: true,
                    });
                }
                None => {
                    return Ok(WholeText {
                        text,
                        truncated: false,
                    });
                }
            }
        }
    }
}

/// Most tokens of one summary written outside the conversation.
const SIDE_REPLY_TOKENS: u32 = 1024;
/// Text one side call takes when the model's window is not known.
const DEFAULT_SIDE_INPUT_BYTES: usize = 24 * 1024;
/// Most text one side call takes, however large the window.
const MAX_SIDE_INPUT_BYTES: usize = 96 * 1024;

/// Model calls outside the conversation (`summarize_file`): the chat role's model with a fresh
/// context, no history and no tools.
pub(crate) struct ModelSideCalls<'a, G> {
    generator: &'a G,
    input_bytes: usize,
    cancellation: CancellationToken,
    /// Whether the person stopped the turn, for hosts that track it outside the token.
    stopped: Option<Box<dyn Fn() -> bool + Send + Sync + 'a>>,
}

impl<'a, G: ChatGenerator> ModelSideCalls<'a, G> {
    /// Side calls sized to the chat model's window, stopped by `cancellation`.
    pub(crate) async fn new(generator: &'a G, cancellation: CancellationToken) -> Self {
        let window = generator.context_window(LlmRole::Chat).await;
        Self {
            generator,
            input_bytes: side_input_bytes(window),
            cancellation,
            stopped: None,
        }
    }

    /// Also stops before a call once `stopped` says the person stopped the turn.
    pub(crate) fn stopped_when(mut self, stopped: impl Fn() -> bool + Send + Sync + 'a) -> Self {
        self.stopped = Some(Box::new(stopped));
        self
    }
}

/// The text one side call takes for a window of `tokens`: about two bytes a token (text runs
/// three to four), less room for the instructions and the summary.
fn side_input_bytes(tokens: Option<u32>) -> usize {
    tokens.map_or(DEFAULT_SIDE_INPUT_BYTES, |tokens| {
        (tokens as usize)
            .saturating_sub(SIDE_REPLY_TOKENS as usize + 1024)
            .saturating_mul(2)
            .clamp(8 * 1024, MAX_SIDE_INPUT_BYTES)
    })
}

#[async_trait]
impl<G: ChatGenerator> SideModel for ModelSideCalls<'_, G> {
    fn input_bytes(&self) -> usize {
        self.input_bytes
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String, SideModelError> {
        if self.cancellation.is_cancelled()
            || self.stopped.as_ref().is_some_and(|stopped| stopped())
        {
            return Err(SideModelError("the person stopped the request".to_owned()));
        }
        let reply = self
            .generator
            .generate_for_role(
                LlmRole::Chat,
                LlmChatGeneration {
                    followups: Vec::new(),
                    tools: Vec::new(),
                    system_prompt: system.to_owned(),
                    user_prompt: prompt.to_owned(),
                    images: Vec::new(),
                    maximum_tokens: SIDE_REPLY_TOKENS,
                    context_window: None,
                    temperature: None,
                    reply: ReplyFormat::Text,
                },
                &self.cancellation,
                &mut |_| {},
            )
            .await
            .map_err(|error| {
                tracing::warn!(error = %error, "a model call outside the conversation failed");
                SideModelError(error.to_string())
            })?;
        if reply.trim().is_empty() {
            return Err(SideModelError("the model returned no text".to_owned()));
        }
        Ok(reply)
    }
}

/// What `read_file` is described as here: the folder tools convert documents to text.
pub(crate) const READ_FILE_DESCRIPTION: &str = "Reads a text file, or the text of an EPUB, Word, \
     PDF, PowerPoint or Excel document or a web page (HTML, without its markup). A long file is \
     cut off with a note: continue with the offset it names.";

/// Which skill folders a request may read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillScope {
    /// The person's own folders and the project's, up to its git root (a desktop host).
    HostWide,
    /// Only the working folder's own `.claude/skills` and `.agents/skills` (a browser client of a
    /// server, which must not see the host's home or the folders above the working folder).
    WorkingFolder,
}

/// What an agent knows about the project of one request: its skills and its instructions.
pub(crate) struct ProjectContext {
    /// Skill folders, then the notes that match the question.
    pub(crate) skills: SkillCatalogue,
    /// The prompt section with the `AGENTS.md`/`CLAUDE.md` instructions; empty without any.
    pub(crate) instructions: String,
}

/// The skills and instructions for one request (task 0342). A local working folder: the
/// project's skill folders and instruction files up to its git root, and on a desktop host the
/// person's own. A folder on another machine: only its own skill folders and instruction files,
/// read through its provider (never the remote home or the folders above), plus the person's own
/// local ones on a desktop host. A browser client of a server sees only the working folder's own.
/// `index` remembers the skill headers between requests.
pub(crate) async fn project_context(
    index: &SkillIndex,
    providers: &ProviderRegistry,
    notes: Option<&dyn NotesAccess>,
    working_directory: &str,
    question: &str,
    scope: SkillScope,
) -> ProjectContext {
    // Tests never read the real home folder.
    let home = if cfg!(test) || scope == SkillScope::WorkingFolder {
        None
    } else {
        dirs::home_dir()
    };
    let local = StoreHandle::local();
    let (mut skills, project_instructions) = match path_from_location(working_directory) {
        Some(directory) => {
            let roots = match scope {
                SkillScope::HostWide => SkillRoots {
                    // The person's own folders are added below, for every kind of folder.
                    user: Vec::new(),
                    ..SkillRoots::standard(Some(&directory), home.as_deref(), &[])
                },
                SkillScope::WorkingFolder => {
                    SkillRoots::working_folder(&directory.to_string_lossy())
                }
            };
            let folders = match scope {
                SkillScope::HostWide => project_folders(&directory, home.as_deref()),
                SkillScope::WorkingFolder => vec![directory.clone()],
            };
            let skills = index.discover(&local, &roots).await;
            (
                skills,
                local_instruction_candidates(&local, &directory, &folders),
            )
        }
        None => match ProviderStore::open(providers, working_directory) {
            Some(remote) => {
                let remote = StoreHandle::new(remote);
                let skills = index
                    .discover(&remote, &SkillRoots::working_folder(""))
                    .await;
                let candidates = instruction_candidates(&remote, &[String::new()], |_, name| {
                    (name.to_owned(), Some(name.to_owned()))
                });
                (skills, candidates)
            }
            None => (SkillCatalogue::default(), Vec::new()),
        },
    };
    if let Some(home) = &home {
        let mut user_roots = SkillRoots::standard(None, Some(home), &[]);
        user_roots.project.clear();
        skills.merge(index.discover(&local, &user_roots).await);
    }
    if let Some(notes) = notes {
        skills.add_notes(notes.skills(question).await);
    }
    let mut candidates = home
        .as_deref()
        .map(|home| home_instruction_candidates(&local, home))
        .unwrap_or_default();
    candidates.extend(project_instructions);
    let instructions =
        instructions_section(&read_instructions(&candidates, MAX_INSTRUCTION_BYTES).await);
    ProjectContext {
        skills,
        instructions,
    }
}

/// The folders of the project, outermost first: the working folder and its parents up to the
/// nearest git root, never the home folder or above it.
fn project_folders(directory: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    for folder in directory.ancestors() {
        if Some(folder) == home {
            break;
        }
        folders.push(folder.to_path_buf());
        if folder.join(".git").exists() {
            break;
        }
    }
    if !folders
        .last()
        .is_some_and(|folder| folder.join(".git").exists())
    {
        // No git root: only the working folder itself.
        folders.truncate(1);
    }
    folders.reverse();
    folders
}

/// Instruction files in the project's folders, named relative to the working folder; only the
/// working folder's own can be read further with `read_file`.
fn local_instruction_candidates(
    local: &StoreHandle,
    directory: &Path,
    folders: &[PathBuf],
) -> Vec<InstructionSource> {
    let texts: Vec<String> = folders
        .iter()
        .map(|folder| folder.to_string_lossy().into_owned())
        .collect();
    let working = directory.to_string_lossy().into_owned();
    instruction_candidates(local, &texts, |folder, name| {
        if folder == working {
            return (name.to_owned(), Some(name.to_owned()));
        }
        let depth = Path::new(&working)
            .strip_prefix(folder)
            .map_or(0, |rest| rest.components().count());
        (format!("{}{name}", "../".repeat(depth)), None)
    })
}

/// The person's own instruction files.
fn home_instruction_candidates(local: &StoreHandle, home: &Path) -> Vec<InstructionSource> {
    [".claude/CLAUDE.md", ".agents/AGENTS.md"]
        .into_iter()
        .map(|name| InstructionSource {
            store: local.clone(),
            path: home.join(name).to_string_lossy().into_owned(),
            label: format!("~/{name}"),
            readable_as: None,
        })
        .collect()
}

/// How the chat role's model calls tools: natively once a probe found it can, else the text
/// protocol.
pub(crate) async fn tool_calling(generator: &impl ChatGenerator) -> ToolCalling {
    if generator.native_tool_calling(LlmRole::Chat).await {
        ToolCalling::Native
    } else {
        ToolCalling::Text
    }
}

/// A conversation as a generation request takes it: for the text protocol, all of it rendered as
/// the prompt; natively, the leading user message as the prompt and the rest as structured
/// follow-up messages (task 0341).
pub(crate) fn generation_parts(
    mut messages: Vec<Message>,
    calling: ToolCalling,
) -> (String, Vec<Message>) {
    match calling {
        ToolCalling::Text => (render_text(&messages), Vec::new()),
        ToolCalling::Native => {
            let prompt = match messages.first_mut() {
                Some(Message::User(prompt)) => std::mem::take(prompt),
                _ => return (String::new(), messages),
            };
            messages.remove(0);
            (prompt, messages)
        }
    }
}

/// Why a model call failed, as the agents report it.
pub(crate) fn chat_failure(error: LlmProfileError) -> ChatFailureDto {
    if !matches!(error, LlmProfileError::Cancelled) {
        // The person sees one short message; the log keeps the cause.
        tracing::warn!(error = %error, "the assistant's model call failed");
    }
    match error {
        LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
        LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
        LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
        LlmProfileError::Timeout => ChatFailureDto::Timeout,
        _ => ChatFailureDto::Unavailable,
    }
}

/// A harness note proposal as the wire carries it.
pub(crate) fn note_proposal_dto(proposal: NoteProposal) -> NoteProposalDto {
    NoteProposalDto {
        path: proposal.path,
        draft: NoteDraftDto {
            title: proposal.title,
            file_name: proposal.file_name,
            tags: proposal.tags,
            content: proposal.content,
            redactions: proposal.redactions,
        },
    }
}

/// A drafted note as the harness carries a proposal.
pub(crate) fn note_proposal(path: Option<String>, draft: NoteDraftDto) -> NoteProposal {
    NoteProposal {
        path,
        title: draft.title,
        file_name: draft.file_name,
        tags: draft.tags,
        content: draft.content,
        redactions: draft.redactions,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fm_vfs_local::LocalFileSystemProvider;

    use super::*;
    use crate::agent_host::sandbox::MAX_READ_BYTES;

    fn folder(dir: &Path) -> FolderTools {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        let url = url::Url::from_directory_path(dir.canonicalize().unwrap()).unwrap();
        FolderTools::open(&providers, url.as_str())
    }

    #[tokio::test]
    async fn a_whole_file_is_read_page_by_page_without_the_cut_off_notes() {
        let dir = tempfile::tempdir().unwrap();
        let text = "line of text\n".repeat(MAX_READ_BYTES / 4);
        std::fs::write(dir.path().join("long.txt"), &text).unwrap();
        let tools = folder(dir.path());

        let whole = tools.read_whole("long.txt", 1024 * 1024).await.unwrap();
        assert_eq!(whole.text, text);
        assert!(!whole.truncated);

        let start = tools.read_whole("long.txt", 10).await.unwrap();
        assert_eq!(start.text.len(), MAX_READ_BYTES);
        assert!(start.truncated);

        assert_eq!(
            tools.read_whole("missing.txt", 100).await,
            Err("missing.txt is not available.".to_owned())
        );
    }

    #[test]
    fn a_side_call_takes_about_two_bytes_a_token_within_bounds() {
        assert_eq!(side_input_bytes(None), DEFAULT_SIDE_INPUT_BYTES);
        assert_eq!(side_input_bytes(Some(2048)), 8 * 1024);
        assert_eq!(side_input_bytes(Some(32 * 1024)), (32 * 1024 - 2048) * 2);
        assert_eq!(side_input_bytes(Some(1_000_000)), MAX_SIDE_INPUT_BYTES);
    }
}
