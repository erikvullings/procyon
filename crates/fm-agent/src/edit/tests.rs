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
            closest: Some((2, "gamma delta epsilon".into())),
            trailing: 0,
            indentation: 0
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

#[tokio::test]
async fn edit_accepts_aliases_and_camel_case() {
    let request = run(json!({
        "filePath": "notes.md", "old_string": "old", "newString": "new", "replaceAll": true
    }))
    .await
    .unwrap();
    assert_eq!(
        request,
        EditRequest {
            replace_all: true,
            ..edit("old", "new")
        }
    );
}

#[test]
fn looser_matches_are_unique_and_report_their_tier() {
    assert_eq!(
        match_edit("one  \r\ntwo\r\n", &edit("one\ntwo", "three")),
        Ok(("three\r\n".to_owned(), EditMatch::TrailingWhitespace))
    );
    assert_eq!(
        match_edit("    one\n      two\n", &edit("one\n  two", "three\n  four")),
        Ok(("    three\n      four\n".to_owned(), EditMatch::Indentation))
    );
    let all = EditRequest {
        replace_all: true,
        ..edit("one\ntwo", "three")
    };
    assert!(matches!(
        match_edit("  one\n  two\n  one\n  two\n", &all),
        Err(EditError::NotFound { indentation: 2, .. })
    ));
}

#[tokio::test]
async fn edit_rejects_aliased_duplicates() {
    let error = run(json!({
        "path": "notes.md", "filename": "other.md", "old_text": "old", "new_text": "new"
    }))
    .await
    .unwrap_err();
    assert!(error.contains("argument path was supplied more than once"));
}

#[test]
fn closest_line_is_available_for_a_single_word_typo() {
    let error = match_edit("beta\nalpha\n", &edit("alphx", "gamma")).unwrap_err();
    assert!(error.to_string().contains("closest line is 2: alpha"));
}
