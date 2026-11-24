use serde_json::json;

use super::*;
use crate::skills::SkillCatalogue;
use crate::tool::{AgentTool, Typed};

fn edit(old: &str, new: &str) -> EditRequest {
    EditRequest {
        path: "notes.md".into(),
        old_text: old.into(),
        new_text: new.into(),
        replace_all: false,
    }
}

#[test]
fn a_unique_text_is_replaced_once() {
    let content = "# Title\n\nFirst line.\nSecond line.\n";
    assert_eq!(
        apply_edit(content, &edit("First line.", "Line one.")),
        Ok("# Title\n\nLine one.\nSecond line.\n".to_owned())
    );
}

#[test]
fn missing_text_names_the_closest_line() {
    let content = "alpha beta\ngamma delta epsilon\n";
    let error = apply_edit(content, &edit("gamma delta zeta", "x")).unwrap_err();
    assert_eq!(
        error,
        EditError::NotFound {
            closest: Some((2, "gamma delta epsilon".into()))
        }
    );
    assert!(
        error
            .to_string()
            .contains("closest line is 2: gamma delta epsilon")
    );
}

#[test]
fn repeated_text_needs_more_context_or_replace_all() {
    let content = "a = 1\nb = 1\n";
    assert_eq!(
        apply_edit(content, &edit("= 1", "= 2")),
        Err(EditError::Ambiguous { count: 2 })
    );
    let all = EditRequest {
        replace_all: true,
        ..edit("= 1", "= 2")
    };
    assert_eq!(apply_edit(content, &all), Ok("a = 2\nb = 2\n".to_owned()));
}

#[test]
fn windows_line_endings_are_kept() {
    let content = "one\r\ntwo\r\nthree\r\n";
    assert_eq!(
        apply_edit(content, &edit("one\ntwo", "uno\ndos")),
        Ok("uno\r\ndos\r\nthree\r\n".to_owned())
    );
}

#[test]
fn an_edit_that_changes_nothing_is_refused() {
    assert_eq!(apply_edit("x", &edit("x", "x")), Err(EditError::Unchanged));
}

async fn run(args: serde_json::Value) -> Result<EditRequest, String> {
    let skills = SkillCatalogue::default();
    match AgentTool::<EditRequest>::run(&Typed(EditFile), &ToolContext::new(&skills), args).await {
        ToolOutcome::Interrupt(request) => Ok(request),
        ToolOutcome::Output(output) => Err(output.text),
    }
}

#[tokio::test]
async fn the_tool_hands_the_request_to_the_host_after_checking_it() {
    let request = run(json!({ "path": " src/a.rs ", "old_text": "a", "new_text": "b" }))
        .await
        .unwrap();
    assert_eq!(request.path, "src/a.rs");
    assert!(!request.replace_all);

    let secret = run(json!({ "path": ".env", "old_text": "a", "new_text": "b" })).await;
    assert!(secret.unwrap_err().contains("may hold secrets"));
    let empty = run(json!({ "path": "a.md", "old_text": "", "new_text": "b" })).await;
    assert!(empty.unwrap_err().contains("use write_file"));
    let big = run(json!({
        "path": "a.md",
        "old_text": "a",
        "new_text": "x".repeat(MAX_EDIT_TEXT_BYTES + 1)
    }))
    .await;
    assert!(big.unwrap_err().contains("at most 64 KiB"));
}
