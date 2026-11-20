use super::{ToolCall, parse_tool_call};

#[test]
fn each_tool_parses_from_its_line() {
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"list_dir","path":"src"}"#),
        Some(ToolCall::ListDir { path: "src".into() })
    );
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"read_file","path":"Cargo.toml"}"#),
        Some(ToolCall::ReadFile {
            path: "Cargo.toml".into()
        })
    );
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"grep","pattern":"todo"}"#),
        Some(ToolCall::Grep {
            pattern: "todo".into(),
            path: None
        })
    );
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"grep","pattern":"todo","path":"src"}"#),
        Some(ToolCall::Grep {
            pattern: "todo".into(),
            path: Some("src".into())
        })
    );
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"run_command","command":"cargo test"}"#),
        Some(ToolCall::RunCommand {
            command: "cargo test".into()
        })
    );
}

#[test]
fn a_fenced_or_prefixed_call_is_found() {
    let reply = "I will look.\n```\nTOOL {\"tool\":\"list_dir\",\"path\":\".\"}\n```";
    assert_eq!(
        parse_tool_call(reply),
        Some(ToolCall::ListDir { path: ".".into() })
    );
}

#[test]
fn anything_else_is_a_plain_answer() {
    assert_eq!(parse_tool_call("Command: ls -la"), None);
    assert_eq!(parse_tool_call("Use the TOOL {x} syntax"), None);
    assert_eq!(parse_tool_call(r#"TOOL {"tool":"nope"}"#), None);
    assert_eq!(parse_tool_call("TOOL {not json"), None);
    assert_eq!(parse_tool_call(r#"TOOL {"tool":"read_file"}"#), None);
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"run_command","command":"  "}"#),
        None
    );
}

#[test]
fn a_command_with_a_newline_or_control_byte_is_not_a_tool_call() {
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"run_command","command":"ls\nrm -rf x"}"#),
        None
    );
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"run_command","command":"ls\u0003"}"#),
        None
    );
}

#[test]
fn a_tool_call_wrapped_in_xml_or_prose_is_still_found() {
    let reply = r#"<tool_call> {"tool": "run_command", "command": "du -sh * 2>/dev/null | sort -rh | head -10"} </invoke> </parameter> </tool_call>"#;
    assert_eq!(
        parse_tool_call(reply),
        Some(ToolCall::RunCommand {
            command: "du -sh * 2>/dev/null | sort -rh | head -10".into()
        })
    );
    let fenced = "Let me look.\n```json\n{\"tool\": \"list_dir\", \"path\": \".\"}\n```";
    assert_eq!(
        parse_tool_call(fenced),
        Some(ToolCall::ListDir { path: ".".into() })
    );
}

#[test]
fn the_name_and_arguments_shape_is_accepted_too() {
    assert_eq!(
        parse_tool_call(r#"{"name": "read_file", "arguments": {"path": "Cargo.toml"}}"#),
        Some(ToolCall::ReadFile {
            path: "Cargo.toml".into()
        })
    );
    assert_eq!(
        parse_tool_call(
            r#"<tool_call>{"name":"run_command","arguments":{"command":"ls"}}</tool_call>"#
        ),
        Some(ToolCall::RunCommand {
            command: "ls".into()
        })
    );
}

#[test]
fn braces_inside_strings_do_not_end_the_object_early() {
    assert_eq!(
        parse_tool_call(r#"TOOL {"tool":"grep","pattern":"fn \"{\" x"}"#),
        Some(ToolCall::Grep {
            pattern: "fn \"{\" x".into(),
            path: None
        })
    );
}

#[test]
fn json_without_a_tool_is_a_plain_answer() {
    assert_eq!(parse_tool_call(r#"The config is {"a": 1}."#), None);
}
