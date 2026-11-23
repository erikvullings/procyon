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
    SkillCatalogue, SkillIndex, SkillRoots, StoreHandle, ToolCalling, ToolOutput,
    instruction_candidates, instructions_section, read_instructions, render_text,
};
use fm_transport_dto::{ChatFailureDto, NoteDraftDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;

pub(crate) use self::documents::{Documents, SkillFolderDocuments, is_document};
pub(crate) use self::sandbox::{FileCall, ToolHost};
use self::skill_store::ProviderStore;
use crate::chat::ChatGenerator;
use crate::llm_profiles::{LlmProfileError, LlmRole};

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
}

/// What `read_file` is described as here: the folder tools convert documents to text.
pub(crate) const READ_FILE_DESCRIPTION: &str = "Reads a text file, or the text of an EPUB, Word, \
     PDF, PowerPoint or Excel document. A long file is cut off with a note: continue with the \
     offset it names.";

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
    match error {
        LlmProfileError::ConsentRequired(_) => ChatFailureDto::ConsentRequired,
        LlmProfileError::RoleNotAssigned => ChatFailureDto::RoleNotAssigned,
        LlmProfileError::Cancelled => ChatFailureDto::Cancelled,
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
