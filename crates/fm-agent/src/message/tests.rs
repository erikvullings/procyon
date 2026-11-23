use serde_json::json;

use super::*;

fn call(id: &str) -> ToolCallRecord {
    ToolCallRecord {
        id: id.to_owned(),
        name: "read_file".to_owned(),
        arguments: json!({"path": id}),
    }
}

fn reply(ids: &[&str]) -> Message {
    Message::Assistant {
        text: format!("Reading {ids:?}"),
        calls: ids.iter().map(|id| call(id)).collect(),
    }
}

fn result(id: &str, size: usize) -> Message {
    Message::result(&call(id), "x".repeat(size))
}

/// Every reply is followed by exactly one result per call, in call order.
fn assert_paired(messages: &[Message]) {
    for (index, message) in messages.iter().enumerate() {
        if let Message::Assistant { calls, .. } = message {
            for (offset, call) in calls.iter().enumerate() {
                match messages.get(index + 1 + offset) {
                    Some(Message::ToolResult { call_id, .. }) => assert_eq!(*call_id, call.id),
                    other => panic!("call {} has no result: {other:?}", call.id),
                }
            }
        }
    }
}

#[test]
fn a_conversation_that_fits_is_unchanged() {
    let messages = vec![
        Message::User("Person: hi".into()),
        reply(&["a", "b"]),
        result("a", 10),
        result("b", 10),
        Message::Assistant {
            text: "Done.".into(),
            calls: vec![],
        },
    ];
    assert_eq!(fit_messages(&messages, 10_000), messages);
}

#[test]
fn old_groups_drop_whole_and_the_task_and_newest_result_stay() {
    let mut messages = vec![Message::User("Person: read everything".into())];
    for n in 0..20 {
        let id = format!("c{n}");
        messages.push(reply(&[&id]));
        messages.push(result(&id, 1000));
    }
    let fitted = fit_messages(&messages, 5000);
    assert_eq!(fitted[0], messages[0]);
    assert_eq!(fitted[1], Message::Note(OMITTED_NOTE.into()));
    assert!(matches!(fitted.last(), Some(Message::ToolResult { call_id, .. }) if call_id == "c19"));
    assert!(fitted.len() < messages.len());
    assert_paired(&fitted);
    assert!(fitted.iter().map(Message::size).sum::<usize>() <= 5000 + OMITTED_NOTE.len());
}

#[test]
fn the_newest_group_stays_even_when_it_alone_is_over_the_bound() {
    let messages = vec![
        Message::User("Person: big".into()),
        reply(&["a"]),
        result("a", 50),
        reply(&["b"]),
        result("b", 10_000),
    ];
    let fitted = fit_messages(&messages, 1000);
    assert_eq!(fitted.len(), 4);
    assert!(matches!(&fitted[3], Message::ToolResult { call_id, .. } if call_id == "b"));
    assert_paired(&fitted);
}

#[test]
fn a_call_without_a_result_gets_one_and_results_come_in_call_order() {
    let messages = vec![
        Message::User("Person: go".into()),
        reply(&["a", "b", "c"]),
        result("c", 3),
        result("a", 3),
        Message::User("Person: never mind".into()),
    ];
    let fitted = fit_messages(&messages, 10_000);
    assert_paired(&fitted);
    assert_eq!(
        fitted[3],
        Message::ToolResult {
            call_id: "b".into(),
            name: "read_file".into(),
            text: NOT_RUN_RESULT.into(),
        }
    );
}

#[test]
fn a_result_without_its_reply_becomes_a_note() {
    let messages = vec![result("lost", 4), Message::User("Person: hi".into())];
    let fitted = fit_messages(&messages, 10_000);
    assert_eq!(fitted[0], Message::Note("Tool result:\nxxxx".into()));
}

#[test]
fn a_note_between_a_reply_and_a_late_result_keeps_the_pair_together() {
    let messages = vec![
        Message::User("Person: run it".into()),
        reply(&["cmd"]),
        Message::Note("The person is looking at the command.".into()),
        result("cmd", 5),
    ];
    let fitted = fit_messages(&messages, 10_000);
    assert_paired(&fitted);
    assert_eq!(
        fitted[3],
        Message::Note("The person is looking at the command.".into())
    );
}

#[test]
fn earlier_turns_before_the_task_are_kept_newest_first_when_room_is_left() {
    let messages = vec![
        Message::User("x".repeat(3000)),
        Message::Assistant {
            text: "old answer".into(),
            calls: vec![],
        },
        Message::User("Person: now".into()),
    ];
    let fitted = fit_messages(&messages, 100);
    assert_eq!(fitted.len(), 2);
    assert_eq!(fitted[1], Message::User("Person: now".into()));
}

#[test]
fn text_rendering_labels_replies_and_results() {
    let text = render_text(&[
        Message::User("Person: hi".into()),
        Message::Assistant {
            text: "TOOL {\"name\":\"read_file\",\"arguments\":{}}".into(),
            calls: vec![call("a")],
        },
        Message::result(&call("a"), "content"),
        Message::Note("Go on.".into()),
    ]);
    assert_eq!(
        text,
        "Person: hi\n\nAssistant: TOOL {\"name\":\"read_file\",\"arguments\":{}}\n\n\
         Tool result:\ncontent\n\nGo on.\n\n"
    );
}

#[test]
fn prose_leaves_out_the_call_lines() {
    assert_eq!(
        Message::prose("Let me look.\nTOOL {\"name\":\"x\"}\nTOOLS are fine"),
        "Let me look.\nTOOLS are fine"
    );
    assert_eq!(Message::prose("TOOL {\"name\":\"x\"}"), "");
}
