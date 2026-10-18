//! Verifies the real workspace against the layering rules in §3 and §4 of the
//! coding-agent specification.

use std::path::{Path, PathBuf};
use std::process::Command;

use fm_test_support::architecture::{check, load_workspace_graph};

/// Native libraries that only the separately packaged semantic worker ships.
const WORKER_ONLY_NATIVE_CRATES: [&str; 6] = [
    "zvec-rust",
    "zvec-rust-sys",
    "ort",
    "ort-sys",
    "lattice-inference",
    "pdfium-render",
];

#[test]
fn desktop_gemma_opt_in_does_not_link_worker_only_native_libraries() {
    // A desktop bundle does not carry the worker's dynamic libraries, so
    // linking them into the host makes the released app fail at launch.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for features in ["", "semantic-gemma"] {
        let output = Command::new(
            std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from),
        )
        .current_dir(&workspace)
        .args([
            "tree",
            "-p",
            "fm-desktop",
            "--target",
            "all",
            "-e",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .args(if features.is_empty() {
            Vec::new()
        } else {
            vec!["--features", features]
        })
        .output()
        .expect("failed to run `cargo tree`");
        assert!(
            output.status.success(),
            "`cargo tree` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let tree = String::from_utf8_lossy(&output.stdout);
        let linked: Vec<&str> = tree
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|name| WORKER_ONLY_NATIVE_CRATES.contains(name))
            .collect();
        assert!(
            linked.is_empty(),
            "fm-desktop with features {features:?} links worker-only native crates: {linked:?}"
        );
    }
}

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
