use fm_transport_dto::{
    TerminalAssistKindDto, TerminalAssistRequestDto, TerminalBlockDto, TerminalContextDto,
};

use super::{
    AttachedFile, path_from_location, render_activity, render_context, render_state, state_sections,
};

fn request(context: TerminalContextDto) -> TerminalAssistRequestDto {
    TerminalAssistRequestDto {
        kind: TerminalAssistKindDto::Ask,
        transcript: String::new(),
        request: Some("q".into()),
        working_directory: "file:///tmp/my%20dir".into(),
        root: None,
        skill: None,
        shell: Some("zsh".into()),
        session_id: Some("t1".into()),
        context,
        tool_result: None,
        write_decision: None,
        allow_file_writes: false,
        workspace_id: None,
    }
}

#[test]
fn a_file_location_becomes_a_path_and_others_do_not() {
    assert_eq!(
        path_from_location("file:///tmp/my%20dir"),
        Some("/tmp/my dir".into())
    );
    assert_eq!(path_from_location("sftp://conn/home/me"), None);
    assert_eq!(path_from_location("not a uri"), None);
}

#[test]
fn a_local_shell_reports_system_shell_and_folder() {
    let text = render_context(&request(TerminalContextDto::default()), &[]);
    assert!(text.contains(&format!(
        "Operating system: {} ({})",
        std::env::consts::OS,
        std::env::consts::ARCH
    )));
    assert!(text.contains("Shell: zsh"));
    assert!(text.contains("Working directory: /tmp/my dir"));
}

#[test]
fn a_remote_shell_or_an_omitted_chip_hides_the_system() {
    let mut remote = request(TerminalContextDto::default());
    remote.working_directory = "sftp://conn/home/me".into();
    assert!(!render_context(&remote, &[]).contains("Operating system"));

    let omitted = request(TerminalContextDto {
        omit_environment: true,
        ..TerminalContextDto::default()
    });
    assert!(!render_context(&omitted, &[]).contains("Operating system"));
}

#[test]
fn listing_and_selection_are_bounded_and_empty_parts_are_left_out() {
    let names: Vec<String> = (0..1100).map(|index| format!("file{index}")).collect();
    let text = render_context(
        &request(TerminalContextDto {
            listing: names,
            selected: vec!["file1".into(), "file2".into()],
            ..TerminalContextDto::default()
        }),
        &[],
    );
    assert!(text.contains("Folder listing (1100 names):"));
    assert!(text.contains("- file999\n"));
    assert!(!text.contains("- file1000\n"));
    assert!(text.contains("and 100 more"));
    assert!(text.contains("Selected in the folder: file1, file2"));

    let bare = render_context(&request(TerminalContextDto::default()), &[]);
    assert!(!bare.contains("Folder listing"));
    assert!(!bare.contains("Recent commands"));
    assert!(!bare.contains("Git branch"));
}

#[test]
fn a_folder_inside_a_git_checkout_reports_its_branch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut asked = request(TerminalContextDto::default());
    asked.working_directory = url::Url::from_directory_path(dir.path().join("sub"))
        .unwrap()
        .to_string();
    assert!(render_context(&asked, &[]).contains("Git branch: feature/x"));
    assert!(!render_context(&request(TerminalContextDto::default()), &[]).contains("Git branch"));
}

#[test]
fn recent_commands_keep_the_newest_output_and_lose_secrets() {
    let long = format!("{}TAIL", "a".repeat(10_000));
    let text = render_context(
        &request(TerminalContextDto {
            blocks: vec![
                TerminalBlockDto {
                    command: "echo hi".into(),
                    exit_code: 0,
                    output: "API_KEY=abc123".into(),
                },
                TerminalBlockDto {
                    command: "make".into(),
                    exit_code: 2,
                    output: long,
                },
            ],
            ..TerminalContextDto::default()
        }),
        &[],
    );
    assert!(text.contains("$ echo hi (exit 0)\nAPI_KEY=[redacted]"));
    assert!(text.contains("$ make (exit 2)"));
    assert!(text.contains("TAIL"));
    assert!(text.len() < 12 * 1024);
}

#[test]
fn attached_files_are_shown_by_name() {
    let text = render_context(
        &request(TerminalContextDto::default()),
        &[AttachedFile {
            name: "Cargo.toml".into(),
            text: "[package]".into(),
        }],
    );
    assert!(text.contains("File Cargo.toml:\n[package]"));
}

#[test]
fn a_huge_selection_is_capped() {
    let selected: Vec<String> = (0..500).map(|index| format!("n{index}")).collect();
    let text = render_context(
        &request(TerminalContextDto {
            selected,
            ..TerminalContextDto::default()
        }),
        &[],
    );
    assert!(text.contains("n49"));
    assert!(!text.contains("n50,") && !text.ends_with("n499"));
}

#[test]
fn state_and_activity_are_rendered_apart_and_together() {
    let asked = request(TerminalContextDto {
        blocks: vec![TerminalBlockDto {
            command: "ls".into(),
            exit_code: 0,
            output: "a".into(),
        }],
        ..TerminalContextDto::default()
    });
    let state = render_state(&asked);
    let activity = render_activity(&asked.context.blocks, &[]);
    assert!(state.contains("Working directory: /tmp/my dir"));
    assert!(!state.contains("Recent commands"));
    assert!(activity.starts_with("Recent commands, oldest first:\n$ ls (exit 0)"));
    assert_eq!(render_context(&asked, &[]), format!("{state}\n{activity}"));
    assert_eq!(render_activity(&[], &[]), "");
}

#[test]
fn the_state_comes_in_parts_that_change_independently() {
    let asked = request(TerminalContextDto {
        selected: vec!["a.txt".into()],
        listing: vec!["a.txt".into(), "b.txt".into()],
        other_folder: Some("file:///other".into()),
        ..TerminalContextDto::default()
    });
    let sections = state_sections(&asked);
    let keys: Vec<&str> = sections.iter().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["header", "otherPane", "selection", "listing"]);
    assert!(sections[0].1.contains("Working directory: /tmp/my dir"));
    assert_eq!(sections[1].1, "Other pane folder: /other");
    assert_eq!(sections[2].1, "Selected in the folder: a.txt");
    assert_eq!(sections[3].1, "Folder listing (2 names):\n- a.txt\n- b.txt");

    let bare = state_sections(&request(TerminalContextDto::default()));
    assert_eq!(bare.len(), 1);
}

#[test]
fn a_local_folders_own_files_are_listed_whatever_the_pane_shows() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "x").unwrap();
    std::fs::write(dir.path().join("a.pdf"), "x").unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let mut asked = request(TerminalContextDto {
        listing: vec!["elsewhere.txt".into()],
        ..TerminalContextDto::default()
    });
    asked.working_directory = url::Url::from_directory_path(dir.path())
        .unwrap()
        .to_string();
    let text = render_context(&asked, &[]);
    assert!(text.contains("Folder listing (3 names):\n- a.pdf\n- b.md\n- sub/"));
    assert!(!text.contains("elsewhere.txt"));
}

#[test]
fn a_git_working_tree_adds_its_status_unless_the_system_pill_is_off() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    std::fs::write(root.path().join("new.txt"), "x").unwrap();
    let folder = url::Url::from_directory_path(root.path().canonicalize().unwrap())
        .unwrap()
        .to_string();
    let mut asked = request(TerminalContextDto::default());
    asked.working_directory.clone_from(&folder);

    let sections = state_sections(&asked);
    let git = sections
        .iter()
        .find(|(name, _)| *name == "git")
        .map(|(_, text)| text.as_str());
    assert_eq!(git, Some("Git status:\n?? new.txt"));

    let mut omitted = request(TerminalContextDto {
        omit_environment: true,
        ..TerminalContextDto::default()
    });
    omitted.working_directory = folder;
    assert!(!render_state(&omitted).contains("Git status"));

    // Outside a working tree there is no git part.
    assert!(!render_state(&request(TerminalContextDto::default())).contains("Git status"));
}
