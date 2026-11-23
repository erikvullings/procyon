//! Skill folders and instruction files of a working folder on another machine (an SSH
//! connection), read through the location's VFS provider (task 0342). Paths are relative to the
//! working folder (`""` is the folder itself) and checked piece by piece: `..`, absolute paths and
//! symbolic links are refused, as for the remote file tools, so nothing outside the folder is read.

use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use fm_agent::{SkillStore, StoreEntry, StoreEntryKind, StoreMetadata};
use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_vfs::{EntryRef, FileSystemProvider, ListOptions, ProviderRegistry};
use tokio::io::AsyncReadExt as _;
use tokio_util::sync::CancellationToken;

/// Most entries listed in one folder.
const MAX_LISTED: usize = 500;

/// A working folder reached through its provider.
pub(crate) struct ProviderStore {
    id: String,
    provider: Arc<dyn FileSystemProvider>,
    root: Location,
}

impl ProviderStore {
    /// `None` when no provider serves the location.
    pub(crate) fn open(providers: &ProviderRegistry, working_directory: &str) -> Option<Self> {
        let root = providers.parse(working_directory).ok()?;
        let provider = providers.resolve(&root).ok()?;
        Some(Self {
            id: format!("vfs:{working_directory}"),
            provider,
            root,
        })
    }

    /// The parts of a relative path, or `None` for one that could leave the folder.
    fn parts(path: &str) -> Option<Vec<&str>> {
        if path.starts_with(['/', '\\']) {
            return None;
        }
        let parts: Vec<&str> = path
            .split(['/', '\\'])
            .filter(|part| !part.is_empty() && *part != ".")
            .collect();
        (!parts.contains(&"..")).then_some(parts)
    }

    /// The location of `path` and what it is, without going through a link.
    async fn locate(&self, path: &str) -> Option<(Location, EntrySummary)> {
        let mut location = self.root.clone();
        let mut summary = self.inspect(&location).await?;
        for part in Self::parts(path)? {
            location = location.join(part).ok()?;
            summary = self.inspect(&location).await?;
            if summary.kind == EntryKind::Symlink {
                return None;
            }
        }
        Some((location, summary))
    }

    async fn inspect(&self, location: &Location) -> Option<EntrySummary> {
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        self.provider
            .inspect(&entry, CancellationToken::new())
            .await
            .ok()
    }
}

fn kind(kind: EntryKind) -> StoreEntryKind {
    match kind {
        EntryKind::File => StoreEntryKind::File,
        EntryKind::Directory => StoreEntryKind::Folder,
        _ => StoreEntryKind::Other,
    }
}

#[async_trait]
impl SkillStore for ProviderStore {
    fn id(&self) -> &str {
        &self.id
    }

    async fn list(&self, folder: &str) -> Option<Vec<StoreEntry>> {
        let (location, summary) = self.locate(folder).await?;
        if summary.kind != EntryKind::Directory {
            return None;
        }
        let page = self
            .provider
            .list(
                &location,
                ListOptions {
                    page_size: MAX_LISTED,
                    continuation_token: None,
                },
                CancellationToken::new(),
            )
            .await
            .ok()?;
        Some(
            page.entries
                .into_iter()
                .map(|entry| StoreEntry {
                    kind: kind(entry.kind),
                    name: entry.name,
                })
                .collect(),
        )
    }

    async fn metadata(&self, path: &str) -> Option<StoreMetadata> {
        let (_, summary) = self.locate(path).await?;
        Some(StoreMetadata {
            kind: kind(summary.kind),
            len: summary.size.unwrap_or(0),
            modified: summary.modified_at.map(SystemTime::from),
        })
    }

    async fn read(&self, path: &str, offset: u64, max: usize) -> Option<Vec<u8>> {
        let (location, summary) = self.locate(path).await?;
        if summary.kind != EntryKind::File {
            return None;
        }
        let entry = EntryRef {
            id: EntryId::new(),
            location,
        };
        let cancellation = CancellationToken::new();
        let mut stream = match self
            .provider
            .read_range(&entry, offset, Some(max as u64), cancellation.clone())
            .await
        {
            Ok(stream) => stream,
            // Without random access, read from the start and skip to the offset.
            Err(_) => {
                let mut stream = self.provider.open_read(&entry, cancellation).await.ok()?;
                tokio::io::copy(&mut (&mut stream).take(offset), &mut tokio::io::sink())
                    .await
                    .ok()?;
                stream
            }
        };
        let mut buffer = Vec::new();
        (&mut stream)
            .take(max as u64)
            .read_to_end(&mut buffer)
            .await
            .ok()?;
        Some(buffer)
    }

    async fn real_path(&self, path: &str) -> Option<String> {
        // Links are refused, so a path that exists is its own real path.
        self.locate(path).await?;
        Some(Self::parts(path)?.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use fm_agent::{SkillIndex, SkillRoots, StoreHandle};
    use fm_vfs_local::LocalFileSystemProvider;

    use super::*;
    use crate::agent_host::{ProjectContext, SkillScope, project_context};

    fn registry() -> ProviderRegistry {
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(LocalFileSystemProvider));
        providers
    }

    fn url(path: &std::path::Path) -> String {
        url::Url::from_directory_path(path.canonicalize().unwrap())
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn a_working_folder_reached_through_its_provider_shows_its_own_skills_only() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("project");
        let skill = folder.join(".claude/skills/deploy");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Deploys\n---\nSteps.",
        )
        .unwrap();
        std::fs::write(skill.join("hosts.md"), "web-1").unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("SKILL.md"), "---\ndescription: Outside\n---\n").unwrap();
        std::fs::write(outside.join("secret.md"), "secret").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, folder.join(".claude/skills/linked")).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.md"), skill.join("leak.md")).unwrap();
        }

        let providers = registry();
        let store = StoreHandle::new(ProviderStore::open(&providers, &url(&folder)).unwrap());
        let catalogue = SkillIndex::new()
            .discover(&store, &SkillRoots::working_folder(""))
            .await;
        assert_eq!(catalogue.len(), 1, "a linked folder is not followed");
        let deploy = catalogue.find("deploy").unwrap();
        assert_eq!(deploy.read_body().await.unwrap().unwrap(), "Steps.");
        assert_eq!(deploy.read_file("hosts.md").await.unwrap(), "web-1");
        #[cfg(unix)]
        assert!(deploy.read_file("leak.md").await.is_err());
        assert!(
            deploy
                .read_file("../../../outside/secret.md")
                .await
                .is_err()
        );
    }

    async fn context(folder: &std::path::Path) -> ProjectContext {
        project_context(
            &SkillIndex::new(),
            &registry(),
            None,
            &url(folder),
            "",
            SkillScope::HostWide,
        )
        .await
    }

    #[tokio::test]
    async fn instructions_come_from_the_git_root_down_to_the_working_folder() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let inner = repo.join("crates/app");
        std::fs::create_dir_all(inner.join(".claude")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(root.path().join("AGENTS.md"), "Above the repo.").unwrap();
        std::fs::write(repo.join("AGENTS.md"), "Use pnpm.").unwrap();
        std::fs::write(inner.join(".claude/CLAUDE.md"), "Be brief.").unwrap();

        let section = context(&inner).await.instructions;
        let pnpm = section
            .find("### ../../AGENTS.md\nUse pnpm.")
            .expect(&section);
        let brief = section
            .find("### .claude/CLAUDE.md\nBe brief.")
            .expect(&section);
        assert!(pnpm < brief, "the outer file comes first");
        assert!(!section.contains("Above the repo."));

        // Without a git root only the working folder's own files count.
        std::fs::remove_dir(repo.join(".git")).unwrap();
        let section = context(&inner).await.instructions;
        assert!(!section.contains("Use pnpm."));
        assert!(section.contains("Be brief."));
    }

    #[test]
    fn paths_that_could_leave_the_folder_have_no_parts() {
        assert_eq!(
            ProviderStore::parts(".claude/./skills/x"),
            Some(vec![".claude", "skills", "x"])
        );
        assert_eq!(ProviderStore::parts(""), Some(vec![]));
        assert_eq!(ProviderStore::parts("a/../b"), None);
        assert_eq!(ProviderStore::parts("/etc"), None);
    }
}
