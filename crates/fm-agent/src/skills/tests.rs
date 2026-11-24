use std::fs;

use async_trait::async_trait;

use super::*;
use crate::notes::{NoteProposal, NoteToolCall, NotesAccess};
use crate::skill_tools::{ReadSkill, ReadSkillFile};
use crate::tool::{DocumentError, SkillDocuments, ToolContext, ToolOutcome, TypedTool};

type Outcome = ToolOutcome<NoteProposal>;

async fn read_skill(ctx: &ToolContext<'_>, args: serde_json::Value) -> String {
    let args = serde_json::from_value(args).unwrap();
    let outcome: Outcome = ReadSkill.run_typed(ctx, args).await;
    outcome.expect_text()
}

async fn read_skill_file(ctx: &ToolContext<'_>, args: serde_json::Value) -> String {
    let args = serde_json::from_value(args).unwrap();
    let outcome: Outcome = ReadSkillFile.run_typed(ctx, args).await;
    outcome.expect_text()
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

async fn folder_skill(dir: &Path) -> Skill {
    let store = StoreHandle::local();
    let header = read_skill_header(&store, &text(dir)).await.unwrap();
    Skill {
        name: header.name,
        description: header.description,
        source: SkillSource::User,
        location: SkillLocation::Folder {
            store,
            path: text(dir),
        },
    }
}

/// The local folder of a skill found on the local disk.
fn dir_of(skill: &Skill) -> PathBuf {
    PathBuf::from(skill.folder().expect("a folder skill").1)
}

fn roots(project: &[&Path], user: &[&Path]) -> SkillRoots {
    SkillRoots {
        project: project.iter().map(|path| text(path)).collect(),
        user: user.iter().map(|path| text(path)).collect(),
    }
}

fn write_skill(root: &Path, folder: &str, text: &str) -> PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), text).unwrap();
    dir
}

async fn catalogue(user: &Path, project: &Path) -> SkillCatalogue {
    SkillCatalogue::discover(&StoreHandle::local(), &roots(&[project], &[user])).await
}

#[tokio::test]
async fn the_catalogue_lists_every_source_with_name_description_and_source() {
    let temp = tempfile::tempdir().unwrap();
    let (user, project) = (temp.path().join("user"), temp.path().join("project"));
    write_skill(
        &user,
        "pdf",
        "---\nname: pdf\ndescription: Use when handling PDFs\n---\nBody",
    );
    write_skill(
        &project,
        "deploy",
        "---\ndescription: >-\n  Use when deploying\n  the app\n---\n# Deploy\n",
    );
    write_skill(
        &project,
        "plain",
        "# Plain\n\nFirst paragraph\nof the skill.\n\nMore.",
    );
    fs::create_dir_all(user.join("empty")).unwrap();

    let mut catalogue = catalogue(&user, &project).await;
    catalogue.add_notes(vec![NoteSkill {
        name: "nginx-cert".into(),
        description: "Use when replacing a certificate".into(),
        path: "ops/nginx.md".into(),
    }]);

    let by_name = |name: &str| catalogue.find(name).unwrap();
    assert_eq!(by_name("pdf").source, SkillSource::User);
    assert_eq!(by_name("deploy").description, "Use when deploying the app");
    assert_eq!(
        by_name("plain").description,
        "First paragraph of the skill."
    );
    assert_eq!(by_name("nginx-cert").source, SkillSource::Note);
    assert!(catalogue.find("empty").is_none());

    let prompt = catalogue.prompt_section("");
    assert!(prompt.contains("- pdf: Use when handling PDFs [user]"));
    assert!(prompt.contains("- deploy: Use when deploying the app [project]"));
    assert!(prompt.contains("- nginx-cert: Use when replacing a certificate [note]"));
}

#[tokio::test]
async fn names_collide_project_then_user_then_note() {
    let temp = tempfile::tempdir().unwrap();
    let (user, project) = (temp.path().join("user"), temp.path().join("project"));
    write_skill(&user, "same", "---\ndescription: from user\n---\n");
    write_skill(&project, "same", "---\ndescription: from project\n---\n");
    let mut catalogue = catalogue(&user, &project).await;
    catalogue.add_notes(vec![NoteSkill {
        name: "same".into(),
        description: "from note".into(),
        path: "same.md".into(),
    }]);
    assert_eq!(catalogue.skills().len(), 1);
    assert_eq!(catalogue.find("same").unwrap().description, "from project");
}

#[tokio::test]
async fn the_catalogue_stays_within_its_caps_and_keeps_the_closest_matches() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user");
    for number in 0..120 {
        let description = format!(
            "{} {}",
            "word".repeat(60),
            if number == 99 { "kubernetes" } else { "x" }
        );
        write_skill(
            &user,
            &format!("skill-{number:03}"),
            &format!("---\ndescription: {description}\n---\n"),
        );
    }
    let catalogue = SkillCatalogue::discover(&StoreHandle::local(), &roots(&[], &[&user])).await;

    let prompt = catalogue.prompt_section("how do I use kubernetes");
    assert!(prompt.len() <= MAX_CATALOGUE_BYTES);
    assert!(prompt.matches("\n- ").count() <= MAX_PROMPT_SKILLS);
    assert!(prompt.contains("skill-099"));
    assert!(
        prompt
            .lines()
            .skip(1)
            .all(|line| line.chars().count() < MAX_DESCRIPTION_CHARS + 60)
    );
}

#[test]
fn standard_roots_walk_up_to_the_git_root_and_stop_at_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let repo = home.join("code/app");
    let inner = repo.join("crates/a");
    fs::create_dir_all(&inner).unwrap();
    fs::create_dir_all(repo.join(".git")).unwrap();

    let roots = SkillRoots::standard(Some(&inner), Some(&home), &[PathBuf::from("/extra")]);
    assert_eq!(roots.user[0], text(&home.join(".claude").join("skills")));
    assert_eq!(roots.user[2], "/extra");
    assert!(
        roots
            .project
            .contains(&text(&repo.join(".claude").join("skills")))
    );
    assert!(
        roots
            .project
            .contains(&text(&inner.join(".agents").join("skills")))
    );
    assert!(!roots.project.iter().any(|root| {
        root.starts_with(&text(&home.join("code"))) && !root.starts_with(&text(&repo))
    }));
    assert!(
        !roots
            .project
            .contains(&text(&home.join(".claude").join("skills")))
    );
}

async fn skill_with_files() -> (tempfile::TempDir, Skill) {
    let temp = tempfile::tempdir().unwrap();
    let dir = write_skill(
        temp.path(),
        "docs",
        "---\nname: docs\ndescription: d\n---\nUse the template.\n",
    );
    fs::create_dir_all(dir.join("references")).unwrap();
    fs::write(dir.join("references/guide.md"), "the guide").unwrap();
    fs::write(dir.join("run.sh"), "#!/bin/sh\necho hi").unwrap();
    fs::write(dir.join("server.key"), "private").unwrap();
    fs::write(dir.join("image.bin"), [0u8, 1, 2]).unwrap();
    fs::write(temp.path().join("outside.txt"), "outside").unwrap();
    let skill = folder_skill(&dir).await;
    (temp, skill)
}

#[tokio::test]
async fn files_inside_the_skill_are_read() {
    let (_temp, skill) = skill_with_files().await;
    assert_eq!(
        skill.read_file("references/guide.md").await.unwrap(),
        "the guide"
    );
    assert_eq!(
        skill.read_body().await.unwrap().unwrap(),
        "Use the template.\n"
    );
    let files = skill.list_files().await;
    assert!(files.contains(&"references/guide.md".to_owned()));
    assert!(files.contains(&"run.sh".to_owned()));
    assert!(!files.contains(&"server.key".to_owned()));
    assert!(!files.contains(&"SKILL.md".to_owned()));
}

#[tokio::test]
async fn reading_cannot_leave_the_skill_folder() {
    let (temp, skill) = skill_with_files().await;
    // The folder must live to the end; only the Unix symlink checks below read it.
    #[cfg(not(unix))]
    let _ = &temp;
    for path in [
        "../outside.txt",
        "references/../../outside.txt",
        "/etc/hosts",
        "",
    ] {
        assert!(
            matches!(
                skill.read_file(path).await,
                Err(SkillFileError::NotAvailable(_))
            ),
            "{path} must not be readable"
        );
    }
    assert!(matches!(
        skill.read_file("server.key").await,
        Err(SkillFileError::Secret(_))
    ));
    assert!(matches!(
        skill.read_file("image.bin").await,
        Err(SkillFileError::NotText(_))
    ));
    assert!(matches!(
        skill.read_file("references").await,
        Err(SkillFileError::NotText(_))
    ));
    #[cfg(unix)]
    {
        let dir = dir_of(&skill);
        std::os::unix::fs::symlink(temp.path().join("outside.txt"), dir.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(temp.path(), dir.join("up")).unwrap();
        assert!(matches!(
            skill.read_file("link.txt").await,
            Err(SkillFileError::NotAvailable(_))
        ));
        assert!(matches!(
            skill.read_file("up/outside.txt").await,
            Err(SkillFileError::NotAvailable(_))
        ));
        assert!(
            !skill
                .list_files()
                .await
                .iter()
                .any(|file| file.contains("outside") || file.contains("link"))
        );
    }
}

#[tokio::test]
async fn credential_folders_inside_a_skill_are_not_read() {
    let (_temp, skill) = skill_with_files().await;
    let dir = dir_of(&skill);
    fs::create_dir_all(dir.join(".ssh")).unwrap();
    fs::write(dir.join(".ssh/config"), "Host x").unwrap();
    assert!(matches!(
        skill.read_file(".ssh/config").await,
        Err(SkillFileError::NotAvailable(_))
    ));
}

struct FakeNotes;

#[async_trait]
impl NotesAccess for FakeNotes {
    async fn run(&self, call: NoteToolCall) -> Outcome {
        match call {
            NoteToolCall::Read { path } => {
                ToolOutcome::output("Read note", format!("Note {path}:\nsteps"))
            }
            _ => ToolOutcome::output("Notes", "unused"),
        }
    }
}

#[tokio::test]
async fn read_skill_loads_a_folder_skill_and_a_note_the_same_way() {
    let (_temp, skill) = skill_with_files().await;
    let mut catalogue = SkillCatalogue::default();
    catalogue.add_notes(vec![NoteSkill {
        name: "nginx-cert".into(),
        description: "d".into(),
        path: "ops/nginx.md".into(),
    }]);
    catalogue.push(skill);
    let ctx = ToolContext {
        notes: Some(&FakeNotes),
        ..ToolContext::new(&catalogue)
    };

    let text = read_skill(&ctx, serde_json::json!({"name": "DOCS"})).await;
    assert!(text.contains("Use the template."));
    assert!(text.contains("references/guide.md"));

    let text = read_skill(&ctx, serde_json::json!({"name": "nginx-cert"})).await;
    assert_eq!(text, "Note ops/nginx.md:\nsteps");

    let text = read_skill(&ctx, serde_json::json!({"name": "nope"})).await;
    assert!(text.starts_with("Error: there is no skill called nope. Available: nginx-cert, docs"));
}

#[tokio::test]
async fn read_skill_file_is_sandboxed_and_redacts() {
    let (temp, skill) = skill_with_files().await;
    let dir = dir_of(&skill);
    fs::write(
        dir.join("notes.txt"),
        "token=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
    )
    .unwrap();
    let mut catalogue = SkillCatalogue::default();
    catalogue.push(skill.clone());
    let ctx = ToolContext::new(&catalogue);
    let read = |path: &str| {
        let args = serde_json::json!({"name": "docs", "path": path});
        let ctx = &ctx;
        async move { read_skill_file(ctx, args).await }
    };
    assert_eq!(read("references/guide.md").await, "the guide");
    assert!(
        read("../outside.txt")
            .await
            .starts_with("Error: ../outside.txt is not available.")
    );
    assert!(!read("notes.txt").await.contains("ghp_abcdef"));
    drop(temp);
}

#[tokio::test]
async fn symlinked_and_copied_skill_folders_list_each_name_once() {
    let temp = tempfile::tempdir().unwrap();
    let claude = temp.path().join(".claude/skills");
    let copy = temp.path().join(".agents/skills");
    let linked = temp.path().join("linked/skills");
    write_skill(
        &claude,
        "pdf",
        "---\nname: pdf\ndescription: from claude\n---\n",
    );
    write_skill(&claude, "docx", "---\ndescription: d\n---\n");
    write_skill(
        &copy,
        "pdf",
        "---\nname: pdf\ndescription: from copy\n---\n",
    );
    fs::create_dir_all(linked.parent().unwrap()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&claude, &linked).unwrap();

    let roots = roots(&[], &[&claude, &copy, &linked]);
    let catalogue = SkillCatalogue::discover(&StoreHandle::local(), &roots).await;

    assert_eq!(catalogue.skills().len(), 2);
    assert_eq!(catalogue.find("pdf").unwrap().description, "from claude");
    assert_eq!(catalogue.prompt_section("").matches("- pdf:").count(), 1);
}

/// A converter that records the paths it was given.
struct RecordingDocuments {
    seen: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl SkillDocuments for RecordingDocuments {
    async fn read(
        &self,
        _store: &StoreHandle,
        path: &str,
        offset: usize,
    ) -> Option<Result<String, DocumentError>> {
        self.seen.lock().unwrap().push(path.to_owned());
        path.ends_with(".pdf")
            .then(|| Ok(format!("pdf text from {offset}")))
    }
}

#[tokio::test]
async fn documents_reach_the_host_converter_only_after_the_sandbox() {
    let (temp, skill) = skill_with_files().await;
    let dir = dir_of(&skill);
    fs::write(dir.join("manual.pdf"), "%PDF").unwrap();
    fs::write(temp.path().join("outside.pdf"), "%PDF").unwrap();
    let mut catalogue = SkillCatalogue::default();
    catalogue.push(skill.clone());
    let documents = RecordingDocuments {
        seen: std::sync::Mutex::new(Vec::new()),
    };
    let ctx = ToolContext {
        documents: Some(&documents),
        ..ToolContext::new(&catalogue)
    };
    let args = |path: &str, offset: usize| serde_json::json!({"name": "docs", "path": path, "offset": offset});

    assert_eq!(
        read_skill_file(&ctx, args("manual.pdf", 7)).await,
        "pdf text from 7"
    );
    for refused in ["../outside.pdf", "server.key", "/etc/hosts"] {
        assert!(
            read_skill_file(&ctx, args(refused, 0))
                .await
                .starts_with("Error:")
        );
    }
    let seen = documents.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].ends_with("manual.pdf"));
}

#[tokio::test]
async fn a_long_skill_file_is_continued_from_the_offset_it_names() {
    let (_temp, skill) = skill_with_files().await;
    let dir = dir_of(&skill);
    let text = "é".repeat(MAX_SKILL_BYTES); // two bytes each: twice the limit
    fs::write(dir.join("long.md"), &text).unwrap();
    let first = skill.read_file("long.md").await.unwrap();
    let marker = "[truncated: continue with offset ";
    let at = first.find(marker).expect("a continuation note");
    let offset: usize = first[at + marker.len()..]
        .trim_end_matches(']')
        .parse()
        .unwrap();
    assert_eq!(offset, MAX_SKILL_BYTES);
    let path = skill.resolve_file("long.md").await.unwrap();
    let rest = skill.read_checked(&path, offset).await.unwrap();
    assert!(rest.starts_with('é'));
    assert_eq!(
        first[..at].trim_end().len() + rest.find('\n').unwrap_or(rest.len()),
        text.len()
    );
}

#[tokio::test]
async fn the_index_reads_a_skill_again_only_when_it_changed() {
    let temp = tempfile::tempdir().unwrap();
    let user = temp.path().join("user");
    let dir = write_skill(
        &user,
        "pdf",
        "---
description: first
---
",
    );
    let roots = roots(&[], &[&user]);
    let store = StoreHandle::local();
    let index = SkillIndex::new();
    assert_eq!(
        index
            .discover(&store, &roots)
            .await
            .find("pdf")
            .unwrap()
            .description,
        "first"
    );
    // Same size and time: the cached header is used.
    assert_eq!(
        index
            .discover(&store, &roots)
            .await
            .find("pdf")
            .unwrap()
            .description,
        "first"
    );
    fs::write(
        dir.join("SKILL.md"),
        "---
description: second, longer
---
",
    )
    .unwrap();
    assert_eq!(
        index
            .discover(&store, &roots)
            .await
            .find("pdf")
            .unwrap()
            .description,
        "second, longer"
    );
    fs::remove_dir_all(&dir).unwrap();
    assert!(index.discover(&store, &roots).await.find("pdf").is_none());
}

/// A store like a remote connection's: paths relative to the working folder, no links.
struct MemoryStore {
    files: std::collections::HashMap<String, Vec<u8>>,
}

impl MemoryStore {
    fn is_folder(&self, path: &str) -> bool {
        path.is_empty()
            || self
                .files
                .keys()
                .any(|file| file.starts_with(&format!("{path}/")))
    }
}

#[async_trait]
impl crate::store::SkillStore for MemoryStore {
    fn id(&self) -> &str {
        "memory"
    }

    async fn list(&self, folder: &str) -> Option<Vec<crate::store::StoreEntry>> {
        if !self.is_folder(folder) {
            return None;
        }
        let mut entries: Vec<crate::store::StoreEntry> = Vec::new();
        for file in self.files.keys() {
            let Some(rest) = relative_to(folder, file).filter(|rest| !rest.is_empty()) else {
                continue;
            };
            let (name, kind) = match rest.split_once('/') {
                Some((name, _)) => (name, StoreEntryKind::Folder),
                None => (rest, StoreEntryKind::File),
            };
            if !entries.iter().any(|entry| entry.name == name) {
                entries.push(crate::store::StoreEntry {
                    name: name.to_owned(),
                    kind,
                });
            }
        }
        Some(entries)
    }

    async fn metadata(&self, path: &str) -> Option<crate::store::StoreMetadata> {
        let (kind, len) = match self.files.get(path) {
            Some(bytes) => (StoreEntryKind::File, bytes.len() as u64),
            None if self.is_folder(path) => (StoreEntryKind::Folder, 0),
            None => return None,
        };
        Some(crate::store::StoreMetadata {
            kind,
            len,
            modified: None,
        })
    }

    async fn read(&self, path: &str, offset: u64, max: usize) -> Option<Vec<u8>> {
        let bytes = self.files.get(path)?;
        let start = (offset as usize).min(bytes.len());
        Some(bytes[start..(start + max).min(bytes.len())].to_vec())
    }

    async fn real_path(&self, path: &str) -> Option<String> {
        (self.files.contains_key(path) || self.is_folder(path)).then(|| path.to_owned())
    }
}

#[tokio::test]
async fn skills_of_a_remote_working_folder_are_found_and_read_through_its_store() {
    let files = [
        (
            ".claude/skills/deploy/SKILL.md",
            "---\ndescription: Use when deploying\n---\nSteps.",
        ),
        (".claude/skills/deploy/ref/hosts.md", "web-1"),
        (".agents/skills/lint/SKILL.md", "# Lint\n\nRun the linter."),
        ("secret.txt", "outside the skill"),
    ];
    let store = StoreHandle::new(MemoryStore {
        files: files
            .iter()
            .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
            .collect(),
    });
    let index = SkillIndex::new();
    let catalogue = index
        .discover(&store, &SkillRoots::working_folder(""))
        .await;
    assert_eq!(catalogue.len(), 2);
    let deploy = catalogue.find("deploy").unwrap();
    assert_eq!(deploy.source, SkillSource::Project);
    assert_eq!(
        catalogue.find("lint").unwrap().description,
        "Run the linter."
    );
    assert_eq!(deploy.read_body().await.unwrap().unwrap(), "Steps.");
    assert_eq!(deploy.list_files().await, ["ref/hosts.md"]);
    assert_eq!(deploy.read_file("ref/hosts.md").await.unwrap(), "web-1");
    assert!(matches!(
        deploy.read_file("../../../secret.txt").await,
        Err(SkillFileError::NotAvailable(_))
    ));
    // Without a modification time, the header is read again each time rather than trusted.
    let again = index
        .discover(&store, &SkillRoots::working_folder(""))
        .await;
    assert_eq!(again.len(), 2);
}
