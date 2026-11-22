use super::*;

fn fields(text: &str) -> FrontMatter {
    split_front_matter(text).0.expect("front matter")
}

#[test]
fn plain_quoted_and_commented_values_are_read() {
    let front = fields(
        "---\nname: pdf-tools\ndescription: \"Use when \\\"merging\\\" PDFs\"\nauthor: 'it''s me'\nversion: 2 # note\n---\nbody",
    );
    assert_eq!(front.get("name"), Some("pdf-tools"));
    assert_eq!(front.get("description"), Some("Use when \"merging\" PDFs"));
    assert_eq!(front.get("author"), Some("it's me"));
    assert_eq!(front.get("version"), Some("2"));
}

#[test]
fn block_scalars_fold_and_keep_line_breaks() {
    let front = fields(
        "---\nname: a\ndescription: >-\n  Use when the person\n  wants a summary.\n\n  Second paragraph.\nnotes: |\n  line one\n  line two\n---\n",
    );
    assert_eq!(
        front.get("description"),
        Some("Use when the person wants a summary.\nSecond paragraph.")
    );
    assert_eq!(front.get("notes"), Some("line one\nline two"));
}

#[test]
fn a_plain_value_can_continue_on_indented_lines() {
    let front = fields("---\ndescription: Use when the\n  value wraps\nname: x\n---\n");
    assert_eq!(front.get("description"), Some("Use when the value wraps"));
    assert_eq!(front.get("name"), Some("x"));
}

#[test]
fn nested_maps_and_lists_do_not_disturb_the_next_key() {
    let front = fields(
        "---\nmetadata:\n  author: someone\n  tags:\n    - a\nallowed-tools:\n- Read\n- Bash\nname: after\n---\n",
    );
    assert_eq!(front.get("metadata"), None);
    assert_eq!(front.get("author"), None);
    assert_eq!(front.get("name"), Some("after"));
}

#[test]
fn without_front_matter_everything_is_body() {
    let (front, body) = split_front_matter("# Title\n---\nnot front matter\n");
    assert!(front.is_none());
    assert!(body.starts_with("# Title"));
    let (front, _) = split_front_matter("---\nname: unterminated\n");
    assert!(front.is_none());
}

#[test]
fn crlf_and_a_byte_order_mark_are_tolerated() {
    let (front, body) = split_front_matter("\u{feff}---\r\nname: win\r\n---\r\nbody");
    assert_eq!(front.unwrap().get("name"), Some("win"));
    assert_eq!(body, "body");
}
