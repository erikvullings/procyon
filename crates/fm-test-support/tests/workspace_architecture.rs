//! Verifies the real workspace against the layering rules in §3 and §4 of the
//! coding-agent specification.

use std::path::Path;

use fm_test_support::architecture::{check, load_workspace_graph};

#[test]
fn workspace_crates_respect_the_documented_layering() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let graph = load_workspace_graph(manifest_dir);

    let violations = check(&graph);

    assert!(
        violations.is_empty(),
        "workspace architecture violations:\n{}",
        violations
            .iter()
            .map(|violation| format!("  - {violation}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
