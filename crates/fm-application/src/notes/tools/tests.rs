use std::fs;

use crate::notes::format::split_note;
use crate::notes::store::NotesStore;
use crate::notes::store::tests::{location_of, registry};
use crate::terminal_assist::{NoteToolCall, NoteToolOutcome, NotesAccess};

use super::AssistantNotes;

async fn notes() -> (tempfile::TempDir, AssistantNotes<'static>) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("ops")).unwrap();
    fs::write(
        dir.path().join("ops/nginx.md"),
        "---\ntitle: Replace nginx certificate\ntags: [nginx]\n---\n# Replace\n\
         export API_TOKEN=abc123\nsudo nginx -t\n",
    )
    .unwrap();
    let store = NotesStore::open(&registry(), &location_of(dir.path()))
        .await
        .unwrap();
    (
        dir,
        AssistantNotes {
            store: Some(store),
            semantic: None,
        },
    )
}

fn text_of(outcome: NoteToolOutcome) -> (String, String) {
    match outcome {
        NoteToolOutcome::Output { step, text } => (step, text),
        NoteToolOutcome::Proposal(proposal) => panic!("unexpected proposal {proposal:?}"),
    }
}

#[tokio::test]
async fn without_a_notes_folder_every_tool_explains_how_to_set_it_up() {
    let notes = AssistantNotes {
        store: None,
        semantic: None,
    };
    let (_, text) = text_of(
        notes
            .run(NoteToolCall::Search {
                query: "x".into(),
                limit: None,
            })
            .await,
    );
    assert!(text.starts_with("Error: notes are not set up yet."));
}

#[tokio::test]
async fn search_lists_ranked_notes_and_names_the_strategy() {
    let (_dir, notes) = notes().await;
    let (step, text) = text_of(
        notes
            .run(NoteToolCall::Search {
                query: "nginx".into(),
                limit: Some(3),
            })
            .await,
    );
    assert_eq!(step, "Searched notes for nginx");
    assert!(text.starts_with(
        "Notes found by text search:\n1. ops/nginx.md — Replace nginx certificate [nginx]"
    ));
    assert!(!text.contains("abc123"));
    let (_, none) = text_of(
        notes
            .run(NoteToolCall::Search {
                query: "kubernetes".into(),
                limit: None,
            })
            .await,
    );
    assert_eq!(none, "No notes found.");
}

#[tokio::test]
async fn reading_is_redacted_and_limited_to_the_notes_folder() {
    let (dir, notes) = notes().await;
    let (step, text) = text_of(
        notes
            .run(NoteToolCall::Read {
                path: "ops/nginx.md".into(),
            })
            .await,
    );
    assert_eq!(step, "Read note ops/nginx.md");
    assert!(text.starts_with("Note ops/nginx.md:\n---\n"));
    assert!(text.contains("API_TOKEN=[redacted]"));
    fs::write(dir.path().join("../outside-0337.md"), "outside").ok();
    let (_, refused) = text_of(
        notes
            .run(NoteToolCall::Read {
                path: "../outside-0337.md".into(),
            })
            .await,
    );
    assert!(refused.starts_with("Error: ../outside-0337.md is not available"));
}

#[tokio::test]
async fn a_new_note_proposal_gets_front_matter_and_redaction() {
    let (_dir, notes) = notes().await;
    let outcome = notes
        .run(NoteToolCall::Propose {
            path: None,
            title: Some("Create a CSR".into()),
            content: "Run openssl.\npassword=hunter2\n".into(),
        })
        .await;
    let NoteToolOutcome::Proposal(proposal) = outcome else {
        panic!("expected a proposal");
    };
    assert_eq!(proposal.path, None);
    assert_eq!(proposal.draft.title, "Create a CSR");
    assert_eq!(proposal.draft.file_name, "create-a-csr.md");
    assert_eq!(proposal.draft.redactions, 1);
    assert!(!proposal.draft.content.contains("hunter2"));
    let (meta, body) = split_note(&proposal.draft.content);
    let meta = meta.expect("front matter");
    assert_eq!(meta.title.as_deref(), Some("Create a CSR"));
    assert_eq!(meta.source.as_deref(), Some("terminal-assistant"));
    assert!(body.starts_with("# Create a CSR\n\nRun openssl."));
}

#[tokio::test]
async fn an_update_must_name_an_existing_note_and_keeps_the_text_as_written() {
    let (_dir, notes) = notes().await;
    let (_, missing) = text_of(
        notes
            .run(NoteToolCall::Propose {
                path: Some("ops/none.md".into()),
                title: None,
                content: "x".into(),
            })
            .await,
    );
    assert!(missing.starts_with("Error: ops/none.md is not available"));

    let replacement =
        "---\ntitle: Replace nginx certificate\naliases: [tls]\n---\n# Replace\nNew steps";
    let NoteToolOutcome::Proposal(proposal) = notes
        .run(NoteToolCall::Propose {
            path: Some("./ops//nginx.md".into()),
            title: None,
            content: replacement.into(),
        })
        .await
    else {
        panic!("expected a proposal");
    };
    assert_eq!(proposal.path.as_deref(), Some("ops/nginx.md"));
    assert_eq!(proposal.draft.content, format!("{replacement}\n"));
    assert_eq!(proposal.draft.title, "Replace nginx certificate");
}

#[tokio::test]
async fn matching_notes_are_offered_as_skills_with_name_and_description() {
    let (dir, notes) = notes().await;
    fs::write(
        dir.path().join("ops/described.md"),
        "---\ntitle: Rotate logs\nname: rotate-logs\ndescription: Use when logs fill the disk\n---\n# Rotate logs\nlogrotate\n",
    )
    .unwrap();

    let skills = notes.skills("replace nginx certificate").await;
    assert_eq!(skills[0].name, "replace-nginx-certificate");
    assert_eq!(skills[0].path, "ops/nginx.md");
    assert!(!skills[0].description.contains("abc123"));

    let skills = notes.skills("logs disk logrotate").await;
    assert_eq!(skills[0].name, "rotate-logs");
    assert_eq!(skills[0].description, "Use when logs fill the disk");

    assert!(notes.skills("   ").await.is_empty());
    let none = AssistantNotes {
        store: None,
        semantic: None,
    };
    assert!(none.skills("x").await.is_empty());
}
