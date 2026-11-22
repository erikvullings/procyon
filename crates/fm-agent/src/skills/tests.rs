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

fn folder_skill(dir: &Path) -> Skill {
    let header = read_skill_header(dir).unwrap();
    Skill {
        name: header.name,
        description: header.description,
        source: SkillSource::User,
        location: SkillLocation::Folder(dir.to_path_buf()),
    }
}

fn write_skill(root: &Path, folder: &str, text: &str) -> PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), text).unwrap();
    dir
}

fn catalogue(user: &Path, project: &Path) -> SkillCatalogue {
    SkillCatalogue::discover(&SkillRoots {
        project: vec![project.to_path_buf()],
        user: vec![user.to_path_buf()],
    })
}

#[test]
fn the_catalogue_lists_every_source_with_name_description_and_source() {
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

    let mut catalogue = catalogue(&user, &project);
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

#[test]
fn names_collide_project_then_user_then_note() {
    let temp = tempfile::tempdir().unwrap();
    let (user, project) = (temp.path().join("user"), temp.path().join("project"));
    write_skill(&user, "same", "---\ndescription: from user\n---\n");
    write_skill(&project, "same", "---\ndescription: from project\n---\n");
    let mut catalogue = catalogue(&user, &project);
    catalogue.add_notes(vec![NoteSkill {
        name: "same".into(),
        description: "from note".into(),
        path: "same.md".into(),
    }]);
    assert_eq!(catalogue.skills().len(), 1);
    assert_eq!(catalogue.find("same").unwrap().description, "from project");
}

#[test]
fn the_catalogue_stays_within_its_caps_and_keeps_the_closest_matches() {
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
    let catalogue = SkillCatalogue::discover(&SkillRoots {
        project: vec![],
        user: vec![user],
    });

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
    assert_eq!(roots.user[0], home.join(".claude/skills"));
    assert_eq!(roots.user[2], PathBuf::from("/extra"));
    assert!(roots.project.contains(&repo.join(".claude/skills")));
    assert!(roots.project.contains(&inner.join(".agents/skills")));
    assert!(
        !roots
            .project
            .iter()
            .any(|root| root.starts_with(home.join("code")) && !root.starts_with(&repo))
    );
    assert!(!roots.project.contains(&home.join(".claude/skills")));
}

fn skill_with_files() -> (tempfile::TempDir, Skill) {
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
    let skill = folder_skill(&dir);
    (temp, skill)
}

#[test]
fn files_inside_the_skill_are_read() {
    let (_temp, skill) = skill_with_files();
    assert_eq!(skill.read_file("references/guide.md").unwrap(), "the guide");
    assert_eq!(skill.read_body().unwrap().unwrap(), "Use the template.\n");
    let files = skill.list_files();
    assert!(files.contains(&"references/guide.md".to_owned()));
    assert!(files.contains(&"run.sh".to_owned()));
    assert!(!files.contains(&"server.key".to_owned()));
    assert!(!files.contains(&"SKILL.md".to_owned()));
}

#[test]
fn reading_cannot_leave_the_skill_folder() {
    let (temp, skill) = skill_with_files();
    for path in [
        "../outside.txt",
        "references/../../outside.txt",
        "/etc/hosts",
        "",
    ] {
        assert!(
            matches!(skill.read_file(path), Err(SkillFileError::NotAvailable(_))),
            "{path} must not be readable"
        );
    }
    assert!(matches!(
        skill.read_file("server.key"),
        Err(SkillFileError::Secret(_))
    ));
    assert!(matches!(
        skill.read_file("image.bin"),
        Err(SkillFileError::NotText(_))
    ));
    assert!(matches!(
        skill.read_file("references"),
        Err(SkillFileError::NotText(_))
    ));
    #[cfg(unix)]
    {
        let SkillLocation::Folder(dir) = &skill.location else {
            unreachable!()
        };
        std::os::unix::fs::symlink(temp.path().join("outside.txt"), dir.join("link.txt")).unwrap();
        std::os::unix::fs::symlink(temp.path(), dir.join("up")).unwrap();
        assert!(matches!(
            skill.read_file("link.txt"),
            Err(SkillFileError::NotAvailable(_))
        ));
        assert!(matches!(
            skill.read_file("up/outside.txt"),
            Err(SkillFileError::NotAvailable(_))
        ));
        assert!(
            !skill
                .list_files()
                .iter()
                .any(|file| file.contains("outside") || file.contains("link"))
        );
    }
}

#[test]
fn credential_folders_inside_a_skill_are_not_read() {
    let (_temp, skill) = skill_with_files();
    let SkillLocation::Folder(dir) = &skill.location else {
        unreachable!()
    };
    fs::create_dir_all(dir.join(".ssh")).unwrap();
    fs::write(dir.join(".ssh/config"), "Host x").unwrap();
    assert!(matches!(
        skill.read_file(".ssh/config"),
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
    let (_temp, skill) = skill_with_files();
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
    let (temp, skill) = skill_with_files();
    let SkillLocation::Folder(dir) = &skill.location else {
        unreachable!()
    };
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

#[test]
fn symlinked_and_copied_skill_folders_list_each_name_once() {
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

    let roots = SkillRoots {
        project: vec![],
        user: vec![claude, copy, linked],
    };
    let catalogue = SkillCatalogue::discover(&roots);

    assert_eq!(catalogue.skills().len(), 2);
    assert_eq!(catalogue.find("pdf").unwrap().description, "from claude");
    assert_eq!(catalogue.prompt_section("").matches("- pdf:").count(), 1);
}

/// A converter that records the paths it was given.
struct RecordingDocuments {
    seen: std::sync::Mutex<Vec<PathBuf>>,
}

#[async_trait]
impl SkillDocuments for RecordingDocuments {
    async fn read(&self, path: &Path, offset: usize) -> Option<Result<String, DocumentError>> {
        self.seen.lock().unwrap().push(path.to_path_buf());
        path.extension()
            .is_some_and(|extension| extension == "pdf")
            .then(|| Ok(format!("pdf text from {offset}")))
    }
}

#[tokio::test]
async fn documents_reach_the_host_converter_only_after_the_sandbox() {
    let (temp, skill) = skill_with_files();
    let SkillLocation::Folder(dir) = &skill.location else {
        unreachable!()
    };
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

#[test]
fn a_long_skill_file_is_continued_from_the_offset_it_names() {
    let (_temp, skill) = skill_with_files();
    let SkillLocation::Folder(dir) = &skill.location else {
        unreachable!()
    };
    let text = "é".repeat(MAX_SKILL_BYTES); // two bytes each: twice the limit
    fs::write(dir.join("long.md"), &text).unwrap();
    let first = skill.read_file("long.md").unwrap();
    let marker = "[truncated: continue with offset ";
    let at = first.find(marker).expect("a continuation note");
    let offset: usize = first[at + marker.len()..]
        .trim_end_matches(']')
        .parse()
        .unwrap();
    assert_eq!(offset, MAX_SKILL_BYTES);
    let path = skill.resolve_file("long.md").unwrap();
    let rest = skill.read_checked(&path, offset).unwrap();
    assert!(rest.starts_with('é'));
    assert_eq!(
        first[..at].trim_end().len() + rest.find('\n').unwrap_or(rest.len()),
        text.len()
    );
}

#[test]
fn the_index_reads_a_skill_again_only_when_it_changed() {
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
    let roots = SkillRoots {
        project: vec![],
        user: vec![user],
    };
    let index = SkillIndex::new();
    assert_eq!(
        index.discover(&roots).find("pdf").unwrap().description,
        "first"
    );
    // Same size and time: the cached header is used.
    assert_eq!(
        index.discover(&roots).find("pdf").unwrap().description,
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
        index.discover(&roots).find("pdf").unwrap().description,
        "second, longer"
    );
    fs::remove_dir_all(&dir).unwrap();
    assert!(index.discover(&roots).find("pdf").is_none());
}
