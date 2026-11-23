use fm_agent::{Message, ToolCallRecord};
use fm_transport_dto::TerminalBlockDto;

use super::{MAX_RENDERED_BYTES, MAX_SESSIONS, MAX_TURNS, Sessions, conversation_budget};

#[test]
fn a_follow_up_sees_the_earlier_turns() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "what is here?", "Working directory: /a");
    sessions.push_assistant("t1", "A Rust project.", &[]);
    sessions.push_person("t1", "build it", "Working directory: /a");

    let prompt = sessions.render("t1");
    assert!(prompt.contains("Person: what is here?"));
    assert!(prompt.contains("Assistant: A Rust project."));
    assert!(prompt.ends_with("Person: build it\n\n"));
}

#[test]
fn each_person_turn_keeps_the_activity_it_came_with() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "first", "OUTPUT-ONE");
    sessions.push_assistant("t1", "ok", &[]);
    sessions.push_person("t1", "second", "OUTPUT-TWO");

    let prompt = sessions.render("t1");
    assert!(prompt.contains("Context attached by the person:\nOUTPUT-ONE\n\nPerson: first"));
    assert!(prompt.contains("Context attached by the person:\nOUTPUT-TWO\n\nPerson: second"));
}

#[test]
fn state_is_sent_with_the_first_question_and_afterwards_only_what_changed() {
    let sessions = Sessions::default();
    let sections = |dir: &str, listing: &str| {
        vec![
            ("header", format!("Working directory: {dir}")),
            ("listing", listing.to_owned()),
        ]
    };
    let first = sessions.state_update("t1", &sections("/a", "- a.txt"));
    assert_eq!(first, "Terminal state:\nWorking directory: /a\n- a.txt");
    assert_eq!(sessions.state_update("t1", &sections("/a", "- a.txt")), "");

    let moved = sessions.state_update("t1", &sections("/b", "- b.txt"));
    assert_eq!(
        moved,
        "Terminal state changed:\nWorking directory: /b\n- b.txt"
    );
    let listing_only = sessions.state_update("t1", &sections("/b", "- b.txt\n- c.txt"));
    assert_eq!(listing_only, "Terminal state changed:\n- b.txt\n- c.txt");
}

#[test]
fn a_part_that_was_left_out_and_comes_back_is_sent_again() {
    let sessions = Sessions::default();
    let with = vec![("listing", "- a".to_owned())];
    sessions.state_update("t1", &with);
    assert_eq!(sessions.state_update("t1", &[]), "");
    assert_eq!(
        sessions.state_update("t1", &with),
        "Terminal state changed:\n- a"
    );
}
#[test]
fn only_commands_not_sent_before_are_offered_again() {
    let sessions = Sessions::default();
    let block = |command: &str, output: &str| TerminalBlockDto {
        command: command.into(),
        exit_code: 0,
        output: output.into(),
    };
    let first = sessions.unsent_blocks("t1", &[block("ls", "a"), block("pwd", "/")]);
    assert_eq!(first.len(), 2);
    let second = sessions.unsent_blocks(
        "t1",
        &[block("ls", "a"), block("pwd", "/"), block("make", "ok")],
    );
    assert_eq!(second, vec![block("make", "ok")]);
    assert_eq!(sessions.unsent_blocks("t2", &[block("ls", "a")]).len(), 1);
}

#[test]
fn tool_results_follow_the_turn_that_asked_for_them() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "list", "ctx");
    sessions.push_assistant("t1", "TOOL {\"tool\":\"list_dir\",\"path\":\".\"}", &[]);
    sessions.push_tool("t1", "a.txt\nb.txt");

    let prompt = sessions.render("t1");
    assert!(prompt.contains("Assistant: TOOL {\"tool\":\"list_dir\",\"path\":\".\"}"));
    assert!(prompt.ends_with("Tool result:\na.txt\nb.txt\n\n"));
}

#[test]
fn the_oldest_turns_go_first_when_the_prompt_is_too_long() {
    let sessions = Sessions::default();
    let filler = "x".repeat(MAX_RENDERED_BYTES / 2);
    sessions.push_person("t1", &format!("one {filler}"), "");
    sessions.push_assistant("t1", &format!("two {filler}"), &[]);
    sessions.push_person("t1", "three", "");

    let prompt = sessions.render("t1");
    assert!(prompt.len() <= MAX_RENDERED_BYTES + 128);
    assert!(!prompt.contains("one "));
    assert!(prompt.ends_with("Person: three\n\n"));
}

#[test]
fn a_stop_lasts_until_the_next_question() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "a", "");
    assert!(!sessions.is_cancelled("t1"));
    sessions.cancel("t1");
    assert!(sessions.is_cancelled("t1"));
    sessions.push_person("t1", "b", "");
    assert!(!sessions.is_cancelled("t1"));
}

#[test]
fn closing_forgets_the_session_and_the_oldest_is_evicted() {
    let sessions = Sessions::default();
    sessions.push_person("keep", "q", "");
    sessions.close("keep");
    assert_eq!(sessions.render("keep"), "");

    for index in 0..=MAX_SESSIONS {
        sessions.push_person(&format!("s{index}"), "q", "");
    }
    assert_eq!(sessions.render("s0"), "");
    assert!(
        sessions
            .render(&format!("s{MAX_SESSIONS}"))
            .contains("Person: q")
    );
}

#[test]
fn stored_turns_are_pruned_not_only_the_prompt() {
    let sessions = Sessions::default();
    for index in 0..(MAX_TURNS * 3) {
        sessions.push_person("t1", &format!("q{index}"), "");
    }
    assert_eq!(sessions.turn_count("t1"), MAX_TURNS);
    assert!(
        sessions
            .render("t1")
            .ends_with(&format!("Person: q{}\n\n", MAX_TURNS * 3 - 1))
    );
}

#[test]
fn the_last_question_survives_the_answers_and_tool_results_after_it() {
    let sessions = Sessions::default();
    assert_eq!(sessions.last_question("t1"), "");
    sessions.push_person("t1", "renew the certificate", "");
    sessions.push_assistant("t1", "TOOL {}", &[]);
    sessions.push_tool("t1", "output");
    assert_eq!(sessions.last_question("t1"), "renew the certificate");
}

#[test]
fn a_long_run_of_tool_results_keeps_the_question_and_its_context() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "summarise the folder", "Folder listing: a.md, b.md");
    let result = "r".repeat(MAX_RENDERED_BYTES / 4);
    for index in 0..8 {
        sessions.push_assistant("t1", &format!("TOOL read {index}"), &[]);
        sessions.push_tool("t1", &format!("{index} {result}"));
    }

    let prompt = sessions.render("t1");
    assert!(prompt.starts_with("Context attached by the person:\nFolder listing: a.md, b.md"));
    assert!(prompt.contains("Person: summarise the folder"));
    assert!(prompt.contains("Earlier tool results were left out"));
    assert!(prompt.contains("7 rrr"));
    assert!(!prompt.contains("1 rrr"));
    assert!(prompt.len() <= MAX_RENDERED_BYTES + 256);
}

#[test]
fn the_conversation_budget_follows_the_models_window() {
    assert_eq!(conversation_budget(None), MAX_RENDERED_BYTES);
    assert!(conversation_budget(Some(262_144)) > 300 * 1024);
    assert!(conversation_budget(Some(u32::MAX)) <= 900 * 1024);
    assert!(conversation_budget(Some(1_000)) >= MAX_RENDERED_BYTES / 3);
}

fn command_call() -> ToolCallRecord {
    ToolCallRecord {
        id: "c1".into(),
        name: "run_command".into(),
        arguments: serde_json::json!({"command": "ls"}),
    }
}

#[test]
fn the_output_of_an_approved_command_answers_its_call() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "list it", "");
    sessions.push_assistant("t1", "TOOL run ls", &[command_call()]);
    sessions.hold_call("t1", command_call());
    sessions.push_tool("t1", "a.txt");
    let messages = sessions.messages_within("t1", 10_000);
    assert_eq!(
        messages[2],
        Message::ToolResult {
            call_id: "c1".into(),
            name: "run_command".into(),
            text: "a.txt".into(),
        }
    );
    // Once answered, a later message is no longer taken as that call's result.
    sessions.push_tool("t1", "stray");
    let messages = sessions.messages_within("t1", 10_000);
    assert_eq!(messages[3], Message::Note("Tool result:\nstray".into()));
}

#[test]
fn a_new_question_answers_a_command_the_person_did_not_run() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "list it", "");
    sessions.push_assistant("t1", "TOOL run ls", &[command_call()]);
    sessions.hold_call("t1", command_call());
    sessions.push_person("t1", "something else", "");
    let messages = sessions.messages_within("t1", 10_000);
    assert!(matches!(
        &messages[2],
        Message::ToolResult { call_id, text, .. } if call_id == "c1" && text.starts_with("Not run")
    ));
    assert_eq!(messages[3], Message::User("Person: something else".into()));
}
