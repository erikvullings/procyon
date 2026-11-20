use super::{MAX_RENDERED_BYTES, MAX_SESSIONS, MAX_TURNS, Sessions};

#[test]
fn a_follow_up_sees_the_earlier_turns() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "what is here?", "Working directory: /a");
    sessions.push_assistant("t1", "A Rust project.");
    sessions.push_person("t1", "build it", "Working directory: /a");

    let prompt = sessions.render("t1");
    assert!(prompt.contains("Person: what is here?"));
    assert!(prompt.contains("Assistant: A Rust project."));
    assert!(prompt.ends_with("Person: build it\n\n"));
}

#[test]
fn only_the_latest_person_turn_carries_its_context() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "first", "OLD-CONTEXT");
    sessions.push_assistant("t1", "ok");
    sessions.push_person("t1", "second", "NEW-CONTEXT");

    let prompt = sessions.render("t1");
    assert!(!prompt.contains("OLD-CONTEXT"));
    assert!(prompt.contains("Context attached by the person:\nNEW-CONTEXT\n\nPerson: second"));
}

#[test]
fn tool_results_follow_the_turn_that_asked_for_them() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "list", "ctx");
    sessions.push_assistant("t1", "TOOL {\"tool\":\"list_dir\",\"path\":\".\"}");
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
    sessions.push_assistant("t1", &format!("two {filler}"));
    sessions.push_person("t1", "three", "");

    let prompt = sessions.render("t1");
    assert!(prompt.len() <= MAX_RENDERED_BYTES + 128);
    assert!(!prompt.contains("one "));
    assert!(prompt.ends_with("Person: three\n\n"));
}

#[test]
fn steps_count_per_person_turn() {
    let sessions = Sessions::default();
    sessions.push_person("t1", "a", "");
    assert_eq!(sessions.next_step("t1"), 1);
    assert_eq!(sessions.next_step("t1"), 2);
    sessions.push_person("t1", "b", "");
    assert_eq!(sessions.next_step("t1"), 1);
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
