//! Procyon's side of the agent harness (task 0339), shared by the terminal assistant and the chat:
//! file tools over one working folder (local, or remote through its provider, with documents read
//! as text), the skills of a request, and the mapping of harness results to wire DTOs. `fm-agent`
//! knows nothing of providers, settings or DTOs; this module is where they meet.

mod documents;
mod remote;
mod sandbox;

use std::path::PathBuf;

use async_trait::async_trait;
use fm_agent::{
    FileTools, NoteProposal, NotesAccess, SkillCatalogue, SkillIndex, SkillRoots, ToolCalling,
    ToolOutput,
};
use fm_transport_dto::{ChatFailureDto, NoteDraftDto, NoteProposalDto};
use fm_vfs::ProviderRegistry;

pub(crate) use self::documents::{Documents, SkillFolderDocuments, is_document};
pub(crate) use self::sandbox::{FileCall, ToolHost};
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

/// The skills for one request: skill folders, then the notes that match `question`. Blocking work
/// runs off the async threads; `index` remembers the headers between requests.
pub(crate) async fn skills_for(
    index: &std::sync::Arc<SkillIndex>,
    notes: Option<&dyn NotesAccess>,
    working_directory: &str,
    question: &str,
    scope: SkillScope,
) -> SkillCatalogue {
    let directory = path_from_location(working_directory);
    let index = std::sync::Arc::clone(index);
    let mut catalogue = tokio::task::spawn_blocking(move || {
        let roots = match scope {
            SkillScope::HostWide => {
                // Tests never read the real home folder.
                let home = if cfg!(test) { None } else { dirs::home_dir() };
                SkillRoots::standard(directory.as_deref(), home.as_deref(), &[])
            }
            SkillScope::WorkingFolder => {
                let mut roots = SkillRoots::default();
                if let Some(folder) = &directory {
                    roots.project.push(folder.join(".claude").join("skills"));
                    roots.project.push(folder.join(".agents").join("skills"));
                }
                roots
            }
        };
        index.discover(&roots)
    })
    .await
    .unwrap_or_default();
    if let Some(notes) = notes {
        catalogue.add_notes(notes.skills(question).await);
    }
    catalogue
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
