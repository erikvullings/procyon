use serde_json::json;

use super::*;

fn known(name: &str) -> bool {
    matches!(name, "echo" | "read_file")
}

fn calls(reply: &str) -> Vec<RawCall> {
    parse_calls(reply, &known, 8)
}

fn one(reply: &str) -> RawCall {
    let mut found = calls(reply);
    assert_eq!(found.len(), 1, "{reply}: {found:?}");
    found.remove(0)
}

#[test]
fn tool_lines_in_both_object_forms_are_calls_in_order() {
    let found = calls(
        "Let me look.\nTOOL {\"tool\":\"echo\",\"text\":\"a\"}\nTOOL {\"name\":\"read_file\",\
         \"arguments\":{\"path\":\"x\"}}",
    );
    assert_eq!(
        found,
        vec![
            RawCall {
                name: "echo".into(),
                args: json!({"text": "a"})
            },
            RawCall {
                name: "read_file".into(),
                args: json!({"path": "x"})
            },
        ]
    );
}

#[test]
fn tool_lines_stop_at_the_limit_and_skip_unknown_tools() {
    let reply = (0..12)
        .map(|n| format!("TOOL {{\"tool\":\"echo\",\"text\":\"{n}\"}}"))
        .chain(["TOOL {\"tool\":\"rm\",\"path\":\"/\"}".to_owned()])
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(parse_calls(&reply, &known, 8).len(), 8);
    assert!(calls("TOOL {\"tool\":\"rm\",\"path\":\"/\"}").is_empty());
    assert!(calls("TOOLS are nice {\"tool\":\"echo\"} indeed").is_empty());
}

#[test]
fn an_argument_called_tool_or_name_survives_the_name_and_arguments_form() {
    let line = tool_call_line("echo", &json!({"tool": "hammer", "name": "x"}));
    assert_eq!(
        one(&line).args,
        json!({"tool": "hammer", "name": "x"}),
        "{line}"
    );
}

#[test]
fn arguments_sent_as_a_json_string_are_decoded() {
    let call = one(r#"{"name":"echo","arguments":"{\"text\":\"hi\"}"}"#);
    assert_eq!(call.args, json!({"text": "hi"}));
}

#[test]
fn tags_fences_and_pretty_printed_objects_on_their_own_line_are_calls() {
    for reply in [
        "<tool_call>{\"name\":\"echo\",\"arguments\":{\"text\":\"x\"}}</tool_call>",
        "```json\n{\"name\":\"echo\",\"arguments\":{\"text\":\"x\"}}\n```",
        "I will look.\n{\n  \"tool\": \"echo\",\n  \"text\": \"x\"\n}",
        "<tool_call>```json {\"tool\":\"echo\",\"text\":\"x\"}```",
        "Let me look: {\"tool\":\"echo\",\"text\":\"x\"}",
    ] {
        assert_eq!(one(reply).name, "echo", "{reply}");
    }
}

#[test]
fn an_example_call_inside_a_sentence_is_not_run() {
    for reply in [
        "You can write {\"tool\":\"echo\",\"text\":\"x\"} to repeat text, as the docs say.",
        "Use `{\"tool\":\"read_file\",\"path\":\"a\"}` and then stop.",
        "The function is `call:echo{text:\"x\"}` in Gemma's syntax.",
    ] {
        assert!(calls(reply).is_empty(), "{reply}");
    }
}

#[test]
fn code_in_an_answer_is_not_a_call() {
    let reply = "Here is the fix:\n```rust\nfn main() {\n    let name = \"x\";\n}\n```";
    assert!(calls(reply).is_empty());
}

#[test]
fn a_models_own_tag_syntax_with_bare_keys_is_a_call() {
    for reply in [
        r#"<|tool_call>call:echo{text:"hi",times:2}<tool_call|>"#,
        r#"<|tool_call>call:echo{text:<|"|>hi<|"|>,times:2}<tool_call|>"#,
    ] {
        assert_eq!(
            one(reply).args,
            json!({"text": "hi", "times": 2}),
            "{reply}"
        );
    }
    assert!(calls("<|tool_call>call:rm{path:\"/\"}<tool_call|>").is_empty());
}

#[test]
fn braces_inside_strings_do_not_end_the_object() {
    assert_eq!(
        balanced_object(r#"{"a":"}{","b":{"c":1}} tail"#),
        Some(r#"{"a":"}{","b":{"c":1}}"#)
    );
    assert_eq!(balanced_object("{unclosed"), None);
    assert_eq!(balanced_object("x{}"), None);
}
