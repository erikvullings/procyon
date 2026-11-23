//! Project instructions: the `AGENTS.md` and `CLAUDE.md` files an agent is expected to follow,
//! read through a [`SkillStore`] (so a project on another machine works too) and added to the
//! system prompt as one bounded section, most general first.

use std::collections::HashSet;

use crate::redact::redact_secrets;
use crate::store::{StoreEntryKind, StoreHandle, join_path};
use crate::text::truncate_to_bytes;

/// The instruction files looked for in each folder, in this order.
pub const INSTRUCTION_FILE_NAMES: [&str; 3] = ["AGENTS.md", "CLAUDE.md", ".claude/CLAUDE.md"];
/// Most bytes of instructions in one prompt, all files together.
pub const MAX_INSTRUCTION_BYTES: usize = 16 * 1024;

/// A file that may hold instructions.
#[derive(Debug, Clone)]
pub struct InstructionSource {
    /// The store it is read through.
    pub store: StoreHandle,
    /// Its path in the store.
    pub path: String,
    /// How the prompt names it (`CLAUDE.md`, `~/.claude/CLAUDE.md`).
    pub label: String,
    /// The path `read_file` takes for it, when the agent can read the rest that way.
    pub readable_as: Option<String>,
}

/// Instructions that were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    /// How the prompt names the file.
    pub label: String,
    /// Its text, possibly cut.
    pub text: String,
    /// The file's full size, when the text was cut.
    pub cut_from: Option<u64>,
    /// The path `read_file` takes for it.
    pub readable_as: Option<String>,
}

/// The candidates in each of `folders` (give them outermost first), in
/// [`INSTRUCTION_FILE_NAMES`] order. `label` names a candidate for the prompt.
pub fn instruction_candidates(
    store: &StoreHandle,
    folders: &[String],
    label: impl Fn(&str, &str) -> (String, Option<String>),
) -> Vec<InstructionSource> {
    let mut candidates = Vec::with_capacity(folders.len() * INSTRUCTION_FILE_NAMES.len());
    for folder in folders {
        for name in INSTRUCTION_FILE_NAMES {
            let (label_text, readable_as) = label(folder, name);
            candidates.push(InstructionSource {
                store: store.clone(),
                path: join_path(folder, name),
                label: label_text,
                readable_as,
            });
        }
    }
    candidates
}

/// Reads the candidates that exist, in order, within `budget` bytes. A file reached twice (an
/// `AGENTS.md` that links to `CLAUDE.md`) is read once; binary files are skipped; the file that
/// crosses the budget is cut and the ones after it are left out.
pub async fn read_instructions(
    candidates: &[InstructionSource],
    budget: usize,
) -> Vec<InstructionFile> {
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    let mut left = budget;
    for candidate in candidates {
        if left == 0 {
            break;
        }
        let store = &candidate.store;
        let Some(metadata) = store.metadata(&candidate.path).await else {
            continue;
        };
        if metadata.kind != StoreEntryKind::File {
            continue;
        }
        let real = store
            .real_path(&candidate.path)
            .await
            .unwrap_or_else(|| candidate.path.clone());
        if !seen.insert((store.id().to_owned(), real)) {
            continue;
        }
        let Some(bytes) = store.read(&candidate.path, 0, left + 1).await else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let cut = bytes.len() > left;
        let text = truncate_to_bytes(text.trim_end(), left).to_owned();
        if text.trim().is_empty() {
            continue;
        }
        left = left.saturating_sub(text.len());
        files.push(InstructionFile {
            label: candidate.label.clone(),
            text: redact_secrets(&text),
            cut_from: cut.then_some(metadata.len),
            readable_as: candidate.readable_as.clone(),
        });
    }
    files
}

/// The prompt section for the instructions that were read; empty without any.
pub fn instructions_section(files: &[InstructionFile]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut text = String::from(
        "Project instructions (from the instruction files below, most general first; follow them \
         unless the person asks otherwise):",
    );
    for file in files {
        text.push_str(&format!("\n\n### {}\n{}", file.label, file.text));
        if let Some(size) = file.cut_from {
            match &file.readable_as {
                Some(path) => text.push_str(&format!(
                    "\n[Cut short ({size} bytes in all); read_file {path} shows the rest.]"
                )),
                None => text.push_str(&format!("\n[Cut short ({size} bytes in all).]")),
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalStore;

    fn local(path: &std::path::Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn outer_files_come_first_and_a_linked_file_is_read_once() {
        let root = tempfile::tempdir().unwrap();
        let inner = root.path().join("app");
        std::fs::create_dir_all(inner.join(".claude")).unwrap();
        std::fs::write(root.path().join("AGENTS.md"), "Use pnpm.").unwrap();
        std::fs::write(inner.join("CLAUDE.md"), "Run the tests.").unwrap();
        std::fs::write(inner.join(".claude/CLAUDE.md"), "Be brief.").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(inner.join("CLAUDE.md"), inner.join("AGENTS.md")).unwrap();
        let store = StoreHandle::new(LocalStore);
        let candidates = instruction_candidates(
            &store,
            &[local(root.path()), local(&inner)],
            |folder, name| (format!("{}/{name}", crate::store::file_name(folder)), None),
        );
        let files = read_instructions(&candidates, MAX_INSTRUCTION_BYTES).await;
        let texts: Vec<&str> = files.iter().map(|file| file.text.as_str()).collect();
        assert_eq!(texts, ["Use pnpm.", "Run the tests.", "Be brief."]);
        let section = instructions_section(&files);
        assert!(section.starts_with("Project instructions"));
        // The link is met first, so the shared text carries its name.
        assert!(section.contains("### app/AGENTS.md\nRun the tests.") || cfg!(not(unix)));
    }

    #[tokio::test]
    async fn the_budget_cuts_the_file_that_crosses_it_and_says_how_to_read_on() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("AGENTS.md"), "a".repeat(100)).unwrap();
        std::fs::write(root.path().join("CLAUDE.md"), "b".repeat(100)).unwrap();
        let store = StoreHandle::new(LocalStore);
        let candidates = instruction_candidates(&store, &[local(root.path())], |_, name| {
            (name.to_owned(), Some(name.to_owned()))
        });
        let files = read_instructions(&candidates, 150).await;
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].text.len(), 50);
        assert_eq!(files[1].cut_from, Some(100));
        assert!(instructions_section(&files).contains("read_file CLAUDE.md shows the rest"));
        assert!(instructions_section(&[]).is_empty());
    }
}
