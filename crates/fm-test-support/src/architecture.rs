//! Fitness checks for the crate layering mandated by the coding-agent
//! specification (§3 and §4).
//!
//! The specification requires crate dependencies to stay directional and
//! acyclic in the direction
//!
//! ```text
//! domain -> events / vfs traits / plugin API -> operations / providers /
//! metadata / search -> application services -> Axum and Tauri hosts
//! ```
//!
//! Cargo already rejects dependency *cycles*, but it happily accepts an
//! inversion such as `fm-domain` depending on `fm-application`. These checks
//! close that gap, and additionally enforce that host-only and
//! application-boundary dependencies do not leak into library crates.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

/// Layer index of every crate in the workspace.
///
/// A crate may only depend on crates in a *strictly lower* layer. Crates
/// sharing a layer are deliberately kept independent of one another, which is
/// what allows them to be built and tested in isolation.
const CRATE_LAYERS: &[(&str, u8)] = &[
    // Layer 0 - the domain model, dependent on nothing in the workspace.
    ("fm-domain", 0),
    // Layer 1 - contracts expressed in terms of the domain model.
    ("fm-auth-oauth", 1),
    ("fm-credentials", 1),
    ("fm-events", 1),
    ("fm-platform", 1),
    ("fm-plugin-api", 1),
    ("fm-search-acceleration", 1),
    ("fm-ssh", 1),
    ("fm-transport-dto", 1),
    ("fm-vfs", 1),
    // Layer 2 - implementations of those contracts, including the primitive
    // engines a higher engine may be composed from.
    ("fm-archive", 2),
    // `fm-checksum` cannot sit beside `fm-vfs`: it consumes the `fm-vfs`
    // provider contract to stream bytes, so it must be strictly above it. It
    // is in turn strictly below `fm-comparison`, which builds its
    // content-hash comparison mode on top of it (task 0077).
    ("fm-checksum", 2),
    ("fm-connections", 2),
    ("fm-credentials-macos", 2),
    ("fm-credentials-windows", 2),
    ("fm-metadata", 2),
    ("fm-operations", 2),
    ("fm-platform-macos", 2),
    ("fm-platform-windows", 2),
    ("fm-plugin-runtime", 2),
    ("fm-search", 2),
    ("fm-settings", 2),
    ("fm-vcs-status", 2),
    ("fm-vfs-local", 2),
    ("fm-vfs-ftp", 2),
    ("fm-vfs-onedrive", 2),
    ("fm-vfs-s3", 2),
    ("fm-vfs-sftp", 2),
    ("fm-vfs-webdav", 2),
    // Layer 3 - composite engines built from the layer-2 primitives.
    ("fm-comparison", 3),
    // Layer 4 - application services, plus the test-support crate which may
    // build fixtures out of anything below it.
    ("fm-application", 4),
    ("fm-test-support", 4),
    // Layer 5 - hosts.
    ("fm-cli", 5),
    ("fm-desktop", 5),
    ("fm-server", 5),
];

/// Third-party crates that only a host application in `apps/` may depend on.
///
/// Specification §3 rule 4: core engine crates must not depend on Axum or
/// Tauri.
const HOST_ONLY_DEPENDENCIES: &[&str] = &[
    "axum",
    "hyper",
    "tauri",
    "tower-http",
    "utoipa-axum",
    "utoipa-swagger-ui",
];

/// Crates reserved for executables.
///
/// Specification §2.2: `anyhow` only at application boundaries or executables;
/// libraries use `thiserror`.
const APPLICATION_BOUNDARY_DEPENDENCIES: &[&str] = &["anyhow"];

/// A single workspace crate and the crates it depends on.
///
/// Development dependencies are excluded: they may legitimately point
/// "upwards" (an application service testing itself against `fm-test-support`)
/// without breaking the runtime layering.
#[derive(Debug, Clone)]
pub struct CrateNode {
    /// Package name, for example `fm-domain`.
    pub name: String,
    /// Whether the crate lives under `apps/` and is therefore a host binary.
    pub is_application: bool,
    /// Names of the crate's normal and build dependencies.
    pub dependencies: Vec<String>,
}

impl CrateNode {
    /// Builds a library node, for use in tests.
    pub fn library<I, S>(name: &str, dependencies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            name: name.to_owned(),
            is_application: false,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        }
    }

    /// Builds a host-application node, for use in tests.
    pub fn application<I, S>(name: &str, dependencies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            name: name.to_owned(),
            is_application: true,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        }
    }
}

/// The workspace dependency graph.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceGraph {
    /// Every member crate of the workspace.
    pub crates: Vec<CrateNode>,
}

impl WorkspaceGraph {
    /// Builds a graph from an iterator of nodes, in any order.
    pub fn new(crates: impl IntoIterator<Item = CrateNode>) -> Self {
        Self {
            crates: crates.into_iter().collect(),
        }
    }
}

/// A breach of the workspace's architectural rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    /// A crate required by the specification is absent from the workspace.
    MissingCrate {
        /// The absent crate.
        name: String,
    },
    /// A workspace member has no layer assigned in [`CRATE_LAYERS`].
    ///
    /// New crates must be placed in the layering deliberately rather than by
    /// accident, so this is a failure rather than a silent pass.
    UnassignedCrate {
        /// The crate missing a layer assignment.
        name: String,
    },
    /// A crate depends on another crate in the same or a higher layer.
    LayerInversion {
        /// The depending crate.
        from: String,
        /// Layer of the depending crate.
        from_layer: u8,
        /// The depended-upon crate.
        to: String,
        /// Layer of the depended-upon crate.
        to_layer: u8,
    },
    /// A crate depends on something reserved for host applications.
    ForbiddenDependency {
        /// The depending crate.
        crate_name: String,
        /// The dependency that is not allowed here.
        dependency: String,
        /// Why the dependency is restricted.
        reason: &'static str,
    },
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCrate { name } => {
                write!(
                    f,
                    "crate `{name}` is required by the specification but is not a workspace member"
                )
            }
            Self::UnassignedCrate { name } => write!(
                f,
                "crate `{name}` has no layer assigned; add it to CRATE_LAYERS in fm-test-support"
            ),
            Self::LayerInversion {
                from,
                from_layer,
                to,
                to_layer,
            } => write!(
                f,
                "`{from}` (layer {from_layer}) must not depend on `{to}` (layer {to_layer}); \
                 dependencies may only point to a strictly lower layer"
            ),
            Self::ForbiddenDependency {
                crate_name,
                dependency,
                reason,
            } => write!(
                f,
                "`{crate_name}` must not depend on `{dependency}`: {reason}"
            ),
        }
    }
}

/// Returns the architectural layer of a workspace crate, if it has one.
#[must_use]
pub fn layer_of(crate_name: &str) -> Option<u8> {
    CRATE_LAYERS
        .iter()
        .find(|(name, _)| *name == crate_name)
        .map(|(_, layer)| *layer)
}

/// Checks a dependency graph against the workspace's architectural rules.
///
/// Violations are returned sorted, so failure output is stable regardless of
/// the order in which crates were discovered.
#[must_use]
pub fn check(graph: &WorkspaceGraph) -> Vec<Violation> {
    let mut violations = Vec::new();

    let present: BTreeSet<&str> = graph.crates.iter().map(|node| node.name.as_str()).collect();
    for (name, _) in CRATE_LAYERS {
        if !present.contains(name) {
            violations.push(Violation::MissingCrate {
                name: (*name).to_owned(),
            });
        }
    }

    for node in &graph.crates {
        let Some(from_layer) = layer_of(&node.name) else {
            violations.push(Violation::UnassignedCrate {
                name: node.name.clone(),
            });
            continue;
        };

        for dependency in dedup(&node.dependencies) {
            if let Some(to_layer) = layer_of(&dependency)
                && to_layer >= from_layer
            {
                violations.push(Violation::LayerInversion {
                    from: node.name.clone(),
                    from_layer,
                    to: dependency.clone(),
                    to_layer,
                });
            }

            if !node.is_application {
                if HOST_ONLY_DEPENDENCIES.contains(&dependency.as_str()) {
                    violations.push(Violation::ForbiddenDependency {
                        crate_name: node.name.clone(),
                        dependency: dependency.clone(),
                        reason: "core crates must not depend on a transport host (specification §3 rule 4)",
                    });
                }
                if APPLICATION_BOUNDARY_DEPENDENCIES.contains(&dependency.as_str()) {
                    violations.push(Violation::ForbiddenDependency {
                        crate_name: node.name.clone(),
                        dependency,
                        reason: "libraries use `thiserror`; `anyhow` is for executables (specification §2.2)",
                    });
                }
            }
        }
    }

    violations.sort();
    violations
}

fn dedup(dependencies: &[String]) -> Vec<String> {
    let unique: BTreeSet<&String> = dependencies.iter().collect();
    unique.into_iter().cloned().collect()
}

/// Loads the real workspace dependency graph via `cargo metadata`.
///
/// `manifest_dir` may be any directory inside the workspace.
///
/// # Panics
///
/// Panics if `cargo metadata` cannot be run or its output cannot be parsed;
/// both indicate a broken checkout rather than a test failure.
#[must_use]
pub fn load_workspace_graph(manifest_dir: &Path) -> WorkspaceGraph {
    let output = Command::new(cargo_binary())
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(manifest_dir.join("Cargo.toml"))
        .output()
        .expect("failed to run `cargo metadata`");

    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).expect("failed to parse `cargo metadata` output");

    let members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let by_id: HashMap<&str, &Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();

    let crates = members
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .map(|package| CrateNode {
            name: package.name.clone(),
            is_application: is_application(&package.manifest_path),
            dependencies: package
                .dependencies
                .iter()
                .filter(|dependency| dependency.kind.as_deref() != Some("dev"))
                .map(|dependency| dependency.name.clone())
                .collect(),
        })
        .collect();

    WorkspaceGraph { crates }
}

/// Whether a manifest lives under the workspace's `apps/` directory.
fn is_application(manifest_path: &Path) -> bool {
    manifest_path
        .components()
        .any(|component| component.as_os_str() == "apps")
}

/// The cargo executable to shell out to, honouring the one that invoked us.
fn cargo_binary() -> PathBuf {
    std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from)
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    /// `None` for a normal dependency, `Some("dev")` or `Some("build")`
    /// otherwise.
    kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_the_domain_crate_to_the_lowest_layer() {
        assert_eq!(layer_of("fm-domain"), Some(0));
        assert!(layer_of("fm-application") > layer_of("fm-vfs"));
        assert!(layer_of("fm-server") > layer_of("fm-application"));
        assert_eq!(layer_of("serde"), None);
    }

    /// A graph containing several independent chains, deliberately unsorted and
    /// interleaved, with violations in the middle of it. A checker that
    /// implicitly assumed crates arrive in layer order would pass the
    /// single-chain cases below but fail this one.
    #[test]
    fn detects_every_layer_inversion_in_an_unsorted_multi_chain_graph() {
        let graph = WorkspaceGraph::new([
            CrateNode::application("fm-server", ["fm-application", "axum"]),
            CrateNode::library("fm-vfs", ["fm-domain", "fm-application"]),
            CrateNode::library("fm-operations", ["fm-vfs", "fm-domain"]),
            CrateNode::library("fm-domain", Vec::<String>::new()),
            CrateNode::library("fm-application", ["fm-operations", "fm-vfs", "fm-events"]),
            CrateNode::library("fm-events", ["fm-domain", "fm-vfs"]),
            CrateNode::application("fm-cli", ["fm-application", "anyhow"]),
            CrateNode::library("fm-vfs-local", ["fm-vfs", "fm-metadata"]),
        ]);

        let violations = check(&graph);

        // `fm-vfs` (1) -> `fm-application` (4)
        assert!(violations.contains(&Violation::LayerInversion {
            from: "fm-vfs".to_owned(),
            from_layer: 1,
            to: "fm-application".to_owned(),
            to_layer: 4,
        }));
        // `fm-events` (1) -> `fm-vfs` (1), a same-layer edge.
        assert!(violations.contains(&Violation::LayerInversion {
            from: "fm-events".to_owned(),
            from_layer: 1,
            to: "fm-vfs".to_owned(),
            to_layer: 1,
        }));
        // `fm-vfs-local` (2) -> `fm-metadata` (2), a same-layer edge in a
        // different chain.
        assert!(violations.contains(&Violation::LayerInversion {
            from: "fm-vfs-local".to_owned(),
            from_layer: 2,
            to: "fm-metadata".to_owned(),
            to_layer: 2,
        }));

        let inversions = violations
            .iter()
            .filter(|violation| matches!(violation, Violation::LayerInversion { .. }))
            .count();
        assert_eq!(inversions, 3, "unexpected inversions in {violations:#?}");
    }

    #[test]
    fn accepts_dependencies_that_point_strictly_downwards() {
        let graph = WorkspaceGraph::new([
            CrateNode::library("fm-operations", ["fm-vfs", "fm-events", "fm-domain"]),
            CrateNode::library("fm-vfs", ["fm-domain"]),
            CrateNode::library("fm-events", ["fm-domain"]),
            CrateNode::library("fm-domain", ["serde", "uuid"]),
        ]);

        let inversions: Vec<_> = check(&graph)
            .into_iter()
            .filter(|violation| matches!(violation, Violation::LayerInversion { .. }))
            .collect();

        assert!(inversions.is_empty(), "{inversions:#?}");
    }

    #[test]
    fn rejects_host_dependencies_in_library_crates_but_allows_them_in_apps() {
        let graph = WorkspaceGraph::new([
            CrateNode::library("fm-application", ["axum"]),
            CrateNode::application("fm-server", ["axum", "utoipa-swagger-ui"]),
        ]);

        let forbidden: Vec<_> = check(&graph)
            .into_iter()
            .filter(|violation| matches!(violation, Violation::ForbiddenDependency { .. }))
            .collect();

        assert_eq!(forbidden.len(), 1, "{forbidden:#?}");
        assert!(matches!(
            &forbidden[0],
            Violation::ForbiddenDependency { crate_name, dependency, .. }
                if crate_name == "fm-application" && dependency == "axum"
        ));
    }

    #[test]
    fn rejects_anyhow_outside_executables() {
        let graph = WorkspaceGraph::new([
            CrateNode::library("fm-domain", ["anyhow"]),
            CrateNode::application("fm-cli", ["anyhow"]),
        ]);

        let forbidden: Vec<_> = check(&graph)
            .into_iter()
            .filter(|violation| matches!(violation, Violation::ForbiddenDependency { .. }))
            .collect();

        assert_eq!(forbidden.len(), 1, "{forbidden:#?}");
    }

    #[test]
    fn reports_required_crates_that_are_absent() {
        let violations = check(&WorkspaceGraph::default());

        assert_eq!(violations.len(), CRATE_LAYERS.len());
        assert!(violations.contains(&Violation::MissingCrate {
            name: "fm-domain".to_owned(),
        }));
    }

    #[test]
    fn reports_workspace_members_with_no_layer_assignment() {
        let mut crates: Vec<CrateNode> = CRATE_LAYERS
            .iter()
            .map(|(name, _)| CrateNode::library(name, Vec::<String>::new()))
            .collect();
        crates.push(CrateNode::library("fm-surprise", Vec::<String>::new()));

        let violations = check(&WorkspaceGraph::new(crates));

        assert_eq!(
            violations,
            vec![Violation::UnassignedCrate {
                name: "fm-surprise".to_owned(),
            }]
        );
    }

    #[test]
    fn treats_manifests_under_apps_as_host_applications() {
        assert!(is_application(Path::new("/repo/apps/fm-server/Cargo.toml")));
        assert!(!is_application(Path::new(
            "/repo/crates/fm-domain/Cargo.toml"
        )));
    }
}
