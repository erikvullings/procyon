//! Application uninstall support (task 0148): reads a `.app` bundle's
//! `CFBundleIdentifier`/`CFBundleName` from its `Info.plist` and scans a
//! fixed set of well-known macOS locations for files/folders that belong to
//! it, so the user can review and choose what else to send to the Trash
//! alongside the bundle itself.
//!
//! Matching is deliberately conservative (task 0148 acceptance criteria):
//! an exact match against the verified `CFBundleIdentifier` is preferred
//! wherever a candidate can carry one, and the fallback match against the
//! product name requires the candidate's whole file-name segment (stripped
//! of its known extension) to equal the product name - never a substring
//! match - so e.g. a "Slack Helper" folder never matches an app named
//! "Slack". Nothing outside the fixed set of locations below is ever
//! scanned.

use std::path::{Path, PathBuf};

use fm_platform::{ApplicationUninstallPlan, PlatformError, UninstallCandidate};

/// A `.app` bundle's identity, read from `Contents/Info.plist`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BundleInfo {
    /// `CFBundleIdentifier`, when the plist declares one.
    pub bundle_identifier: Option<String>,
    /// `CFBundleName`, falling back to `CFBundleDisplayName`.
    pub product_name: Option<String>,
}

/// Reads `CFBundleIdentifier`/`CFBundleName` (falling back to
/// `CFBundleDisplayName`) from `<bundle_path>/Contents/Info.plist`. Returns
/// an empty [`BundleInfo`] rather than an error when the plist is missing,
/// unreadable, or malformed - callers still have the bundle's own file name
/// to fall back to for product-name matching.
#[must_use]
pub(crate) fn read_bundle_info(bundle_path: &Path) -> BundleInfo {
    let info_plist_path = bundle_path.join("Contents").join("Info.plist");
    let Ok(value) = plist::Value::from_file(&info_plist_path) else {
        return BundleInfo::default();
    };
    let Some(dict) = value.as_dictionary() else {
        return BundleInfo::default();
    };
    let bundle_identifier = dict
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    let product_name = dict
        .get("CFBundleName")
        .or_else(|| dict.get("CFBundleDisplayName"))
        .and_then(plist::Value::as_string)
        .map(str::to_owned);
    BundleInfo {
        bundle_identifier,
        product_name,
    }
}

/// The well-known macOS locations task 0148 scans for an application's
/// related files, rooted so tests can point them at a fixture directory
/// tree instead of the real `~/Library`/`/Library`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UninstallSearchRoots {
    /// `~/Library` (or a fixture standing in for it).
    pub user_library: PathBuf,
    /// `/Library` (or a fixture standing in for it). Only ever listed, never
    /// deleted from: writing there requires elevation, which is out of scope
    /// for this task.
    pub system_library: PathBuf,
}

impl UninstallSearchRoots {
    /// The real `~/Library` and `/Library` rooted at `home`.
    #[must_use]
    pub(crate) fn real(home: &Path) -> Self {
        Self {
            user_library: home.join("Library"),
            system_library: PathBuf::from("/Library"),
        }
    }
}

/// `true` when `bundle_path` sits inside `/Applications` or `~/Applications`
/// (at any depth, so e.g. `/Applications/Utilities/Terminal.app` still
/// counts) - the only locations this feature treats as a genuinely
/// *installed* application, mirroring how a real macOS uninstaller (e.g.
/// ForkLift, AppCleaner) scopes itself. A `.app` bundle elsewhere - a
/// Downloads folder, a project directory the user is actively developing
/// in, `/System/Applications`, ... - is not offered this action: its
/// `~/Library` matches would be far less reliably attributable to it, and
/// SIP-protected system bundles must never be offered for removal at all.
fn is_installed_application(bundle_path: &Path, home: &Path) -> bool {
    [PathBuf::from("/Applications"), home.join("Applications")]
        .iter()
        .any(|root| bundle_path.starts_with(root))
}

/// One well-known scan location: a directory to list, an optional filename
/// suffix to strip before matching (e.g. `.plist`), and whether a match
/// found there may actually be trashed.
struct ScanRule {
    dir: PathBuf,
    strip_suffix: Option<&'static str>,
    removable: bool,
}

fn scan_rules(roots: &UninstallSearchRoots) -> Vec<ScanRule> {
    vec![
        ScanRule {
            dir: roots.user_library.join("Application Support"),
            strip_suffix: None,
            removable: true,
        },
        ScanRule {
            dir: roots.user_library.join("Caches"),
            strip_suffix: None,
            removable: true,
        },
        ScanRule {
            dir: roots.user_library.join("Preferences"),
            strip_suffix: Some(".plist"),
            removable: true,
        },
        ScanRule {
            dir: roots.user_library.join("Saved Application State"),
            strip_suffix: Some(".savedState"),
            removable: true,
        },
        ScanRule {
            dir: roots.user_library.join("LaunchAgents"),
            strip_suffix: Some(".plist"),
            removable: true,
        },
        // Listed only (task 0148): writing to `/Library` requires elevation,
        // out of scope here.
        ScanRule {
            dir: roots.system_library.join("LaunchAgents"),
            strip_suffix: Some(".plist"),
            removable: false,
        },
        ScanRule {
            dir: roots.user_library.join("Logs"),
            strip_suffix: None,
            removable: true,
        },
    ]
}

/// `true` when `stem` (a candidate's file name with its known extension
/// already stripped) identifies the same application as `bundle_identifier`/
/// `product_name`: an exact match against the bundle identifier, or - only
/// when no identifier match exists - a case-insensitive match against the
/// *whole* product name, never a substring match.
fn matches_identity(stem: &str, bundle_identifier: Option<&str>, product_name: &str) -> bool {
    if let Some(identifier) = bundle_identifier
        && stem == identifier
    {
        return true;
    }
    !product_name.is_empty() && stem.eq_ignore_ascii_case(product_name)
}

/// Scans task 0148's well-known locations under `roots` for files/folders
/// whose name (minus a known suffix, e.g. `.plist`) matches
/// `bundle_identifier` exactly or `product_name` as a whole segment.
///
/// Only entries directly inside one of the fixed scan directories are
/// considered - nothing else on disk is ever touched or recursed into for
/// matching purposes (recursion is used only to size an already-matched
/// directory).
#[must_use]
pub(crate) fn discover_related_files(
    roots: &UninstallSearchRoots,
    bundle_identifier: Option<&str>,
    product_name: &str,
) -> Vec<UninstallCandidate> {
    let mut candidates = Vec::new();
    for rule in scan_rules(roots) {
        let Ok(entries) = std::fs::read_dir(&rule.dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let stem = rule
                .strip_suffix
                .and_then(|suffix| file_name.strip_suffix(suffix))
                .unwrap_or(file_name);
            if !matches_identity(stem, bundle_identifier, product_name) {
                continue;
            }
            candidates.push(UninstallCandidate {
                size_bytes: entry_size_bytes(&path),
                path,
                removable: rule.removable,
            });
        }
    }
    candidates
}

/// Sizes a file or symlink directly, and a directory recursively (summing
/// only regular files - never following symlinks encountered inside it, so
/// a crafted symlink loop cannot make this loop or over-report).
fn entry_size_bytes(path: &Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !metadata.is_dir() {
        return metadata.len();
    }
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

/// Builds the full [`ApplicationUninstallPlan`] for `bundle_path`: reads its
/// identity, then scans the real `~/Library`/`/Library` for related files.
/// Nothing is deleted; this only plans.
pub(crate) fn plan_application_uninstall(
    bundle_path: &Path,
) -> Result<ApplicationUninstallPlan, PlatformError> {
    if !bundle_path.is_dir()
        || bundle_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("app")
    {
        return Err(PlatformError::NotFound {
            path: bundle_path.display().to_string(),
        });
    }
    let home = dirs::home_dir().ok_or_else(|| PlatformError::Io {
        message: "home directory is unavailable".to_owned(),
    })?;
    if !is_installed_application(bundle_path, &home) {
        return Err(PlatformError::Io {
            message: "only applications under /Applications or ~/Applications can be uninstalled this way".to_owned(),
        });
    }
    let bundle_info = read_bundle_info(bundle_path);
    let product_name = bundle_info.product_name.clone().unwrap_or_else(|| {
        bundle_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let roots = UninstallSearchRoots::real(&home);
    let related_files = discover_related_files(
        &roots,
        bundle_info.bundle_identifier.as_deref(),
        &product_name,
    );
    Ok(ApplicationUninstallPlan {
        bundle_identifier: bundle_info.bundle_identifier,
        product_name,
        related_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_info_plist(bundle_dir: &Path, identifier: Option<&str>, name: Option<&str>) {
        let contents_dir = bundle_dir.join("Contents");
        fs::create_dir_all(&contents_dir).expect("create Contents dir");
        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n",
        );
        if let Some(identifier) = identifier {
            xml.push_str(&format!(
                "<key>CFBundleIdentifier</key><string>{identifier}</string>\n"
            ));
        }
        if let Some(name) = name {
            xml.push_str(&format!("<key>CFBundleName</key><string>{name}</string>\n"));
        }
        xml.push_str("</dict></plist>");
        fs::write(contents_dir.join("Info.plist"), xml).expect("write Info.plist");
    }

    #[test]
    fn read_bundle_info_extracts_identifier_and_name_from_a_fixture_info_plist() {
        let bundle = tempdir().expect("tempdir");
        write_info_plist(bundle.path(), Some("com.example.Widget"), Some("Widget"));

        let info = read_bundle_info(bundle.path());

        assert_eq!(
            info.bundle_identifier.as_deref(),
            Some("com.example.Widget")
        );
        assert_eq!(info.product_name.as_deref(), Some("Widget"));
    }

    #[test]
    fn read_bundle_info_falls_back_to_display_name_when_bundle_name_is_absent() {
        let contents_dir_holder = tempdir().expect("tempdir");
        let contents_dir = contents_dir_holder.path().join("Contents");
        fs::create_dir_all(&contents_dir).expect("create Contents dir");
        fs::write(
            contents_dir.join("Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\n\
             <key>CFBundleIdentifier</key><string>com.example.Widget</string>\n\
             <key>CFBundleDisplayName</key><string>Widget Pro</string>\n\
             </dict></plist>",
        )
        .expect("write Info.plist");

        let info = read_bundle_info(contents_dir_holder.path());

        assert_eq!(info.product_name.as_deref(), Some("Widget Pro"));
    }

    #[test]
    fn read_bundle_info_returns_empty_for_a_missing_info_plist() {
        let bundle = tempdir().expect("tempdir");

        let info = read_bundle_info(bundle.path());

        assert_eq!(info, BundleInfo::default());
    }

    fn write_fixture_tree(user_library: &Path) {
        fs::create_dir_all(user_library.join("Application Support/Widget")).unwrap();
        fs::create_dir_all(user_library.join("Application Support/WidgetHelper")).unwrap();
        fs::write(
            user_library
                .join("Application Support/Widget")
                .join("settings.json"),
            b"{}",
        )
        .unwrap();
        fs::create_dir_all(user_library.join("Caches/com.example.Widget")).unwrap();
        fs::create_dir_all(user_library.join("Preferences")).unwrap();
        fs::write(
            user_library.join("Preferences/com.example.Widget.plist"),
            b"binary-plist-stand-in",
        )
        .unwrap();
        fs::write(
            user_library.join("Preferences/com.example.WidgetHelper.plist"),
            b"binary-plist-stand-in",
        )
        .unwrap();
    }

    #[test]
    fn discover_related_files_matches_by_exact_bundle_identifier() {
        let home = tempdir().expect("tempdir");
        let user_library = home.path().join("Library");
        write_fixture_tree(&user_library);
        let roots = UninstallSearchRoots {
            user_library,
            system_library: home.path().join("SystemLibraryUnused"),
        };

        let found = discover_related_files(&roots, Some("com.example.Widget"), "Widget");

        let paths: Vec<String> = found
            .iter()
            .map(|candidate| candidate.path.display().to_string())
            .collect();
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("Application Support/Widget"))
        );
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("Caches/com.example.Widget"))
        );
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("Preferences/com.example.Widget.plist"))
        );
    }

    #[test]
    fn discover_related_files_does_not_match_a_similarly_named_helper_folder() {
        // False-positive guard: "WidgetHelper" must never match "Widget" -
        // neither the identifier match (different identifier entirely) nor
        // the product-name fallback (whole-segment, not substring).
        let home = tempdir().expect("tempdir");
        let user_library = home.path().join("Library");
        write_fixture_tree(&user_library);
        let roots = UninstallSearchRoots {
            user_library,
            system_library: home.path().join("SystemLibraryUnused"),
        };

        let found = discover_related_files(&roots, Some("com.example.Widget"), "Widget");

        let paths: Vec<String> = found
            .iter()
            .map(|candidate| candidate.path.display().to_string())
            .collect();
        assert!(!paths.iter().any(|path| path.contains("WidgetHelper")));
    }

    #[test]
    fn discover_related_files_falls_back_to_a_whole_segment_product_name_match() {
        let home = tempdir().expect("tempdir");
        let user_library = home.path().join("Library");
        fs::create_dir_all(user_library.join("Application Support/Widget")).unwrap();
        fs::create_dir_all(user_library.join("Application Support/WidgetHelper")).unwrap();
        let roots = UninstallSearchRoots {
            user_library,
            system_library: home.path().join("SystemLibraryUnused"),
        };

        // No bundle identifier available at all - product-name-only fallback.
        let found = discover_related_files(&roots, None, "Widget");

        let paths: Vec<String> = found
            .iter()
            .map(|candidate| candidate.path.display().to_string())
            .collect();
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("Application Support/Widget"))
        );
        assert!(!paths.iter().any(|path| path.contains("WidgetHelper")));
    }

    #[test]
    fn discover_related_files_reports_system_library_matches_as_not_removable() {
        let home = tempdir().expect("tempdir");
        let user_library = home.path().join("Library");
        let system_library = home.path().join("SystemLibrary");
        fs::create_dir_all(user_library.join("LaunchAgents")).unwrap();
        fs::create_dir_all(system_library.join("LaunchAgents")).unwrap();
        fs::write(
            user_library.join("LaunchAgents/com.example.Widget.plist"),
            b"plist",
        )
        .unwrap();
        fs::write(
            system_library.join("LaunchAgents/com.example.Widget.plist"),
            b"plist",
        )
        .unwrap();
        let roots = UninstallSearchRoots {
            user_library,
            system_library,
        };

        let found = discover_related_files(&roots, Some("com.example.Widget"), "Widget");

        let user_agent = found
            .iter()
            .find(|candidate| {
                candidate.path.starts_with(&roots.user_library)
                    && candidate.path.ends_with("com.example.Widget.plist")
            })
            .expect("user LaunchAgents match");
        assert!(user_agent.removable);
        let system_agent = found
            .iter()
            .find(|candidate| candidate.path.starts_with(&roots.system_library))
            .expect("system LaunchAgents match");
        assert!(!system_agent.removable);
    }

    #[test]
    fn discover_related_files_sums_directory_sizes_recursively() {
        let home = tempdir().expect("tempdir");
        let user_library = home.path().join("Library");
        let app_support = user_library.join("Application Support/Widget");
        fs::create_dir_all(app_support.join("nested")).unwrap();
        fs::write(app_support.join("a.txt"), vec![0u8; 10]).unwrap();
        fs::write(app_support.join("nested/b.txt"), vec![0u8; 20]).unwrap();
        let roots = UninstallSearchRoots {
            user_library,
            system_library: home.path().join("SystemLibraryUnused"),
        };

        let found = discover_related_files(&roots, Some("com.example.Widget"), "Widget");

        let candidate = found
            .iter()
            .find(|candidate| candidate.path.ends_with("Application Support/Widget"))
            .expect("Application Support match");
        assert_eq!(candidate.size_bytes, 30);
    }

    #[test]
    fn plan_application_uninstall_rejects_a_path_that_is_not_an_app_bundle() {
        let not_a_bundle = tempdir().expect("tempdir");

        let error = plan_application_uninstall(not_a_bundle.path())
            .expect_err("a plain directory is not a .app bundle");

        assert!(matches!(error, PlatformError::NotFound { .. }));
    }

    #[test]
    fn plan_application_uninstall_rejects_a_real_bundle_outside_applications() {
        // A tempdir is never under /Applications or ~/Applications, so a real,
        // otherwise-valid bundle placed there must still be rejected (task 0148 follow-up:
        // scope this feature to genuinely installed applications).
        let scratch = tempdir().expect("tempdir");
        let bundle_path = scratch.path().join("Widget.app");
        write_info_plist(&bundle_path, Some("com.example.Widget"), Some("Widget"));

        let error = plan_application_uninstall(&bundle_path)
            .expect_err("a bundle outside /Applications and ~/Applications must be rejected");

        assert!(matches!(error, PlatformError::Io { .. }));
    }

    #[test]
    fn is_installed_application_accepts_the_global_and_per_user_applications_folders() {
        let home = PathBuf::from("/Users/erik");
        assert!(is_installed_application(
            Path::new("/Applications/Widget.app"),
            &home
        ));
        // Nested one level deep, e.g. the real /Applications/Utilities on every Mac.
        assert!(is_installed_application(
            Path::new("/Applications/Utilities/Widget.app"),
            &home
        ));
        assert!(is_installed_application(
            &home.join("Applications/Widget.app"),
            &home
        ));
    }

    #[test]
    fn is_installed_application_rejects_bundles_outside_the_trusted_roots() {
        let home = PathBuf::from("/Users/erik");
        assert!(!is_installed_application(
            &home.join("Downloads/Widget.app"),
            &home
        ));
        assert!(!is_installed_application(
            &home.join("Projects/Widget/build/Widget.app"),
            &home
        ));
        // SIP-protected system apps must never be offered for removal by this feature.
        assert!(!is_installed_application(
            Path::new("/System/Applications/Widget.app"),
            &home
        ));
        assert!(!is_installed_application(
            Path::new("/Volumes/External/Widget.app"),
            &home
        ));
    }
}
