//! The "New file from template" catalog (task 0312): app templates from settings, text files in
//! the user's template folder, and templates contributed by enabled plugins, in that order.

use std::path::Path;

use fm_settings::FileTemplate;
use fm_transport_dto::{FileTemplateEntryDto, FileTemplateSourceDto};

/// Most entries read from the template folder, after sorting by file name.
const MAX_FOLDER_TEMPLATES: usize = 30;
/// Largest folder file offered as a template, in bytes.
const MAX_FOLDER_TEMPLATE_BYTES: u64 = 64 * 1024;

/// Builds the catalog: enabled app templates, then the folder's files, then plugin templates.
pub(crate) fn catalog(
    file_templates: &[FileTemplate],
    template_folder: Option<&str>,
    plugin_templates: Vec<FileTemplateEntryDto>,
) -> Vec<FileTemplateEntryDto> {
    let app = file_templates
        .iter()
        .filter(|template| template.enabled)
        .map(|template| FileTemplateEntryDto {
            id: template.id.clone(),
            label: template.label.clone(),
            file_name: template.file_name.clone(),
            body: template.body.clone(),
            source: FileTemplateSourceDto::App,
            source_name: None,
        });
    let folder = template_folder
        .map(|folder| folder_templates(&expand_home(folder)))
        .unwrap_or_default();
    app.chain(folder).chain(plugin_templates).collect()
}

/// Resolves a leading `~` (alone or as `~/…`) against the user's home directory.
fn expand_home(folder: &str) -> std::path::PathBuf {
    let home = || crate::workspace::resolve_home_directory();
    match folder.strip_prefix("~") {
        Some("") => home(),
        Some(rest) if rest.starts_with(['/', '\\']) => home().join(&rest[1..]),
        _ => std::path::PathBuf::from(folder),
    }
}

/// The regular, non-hidden, UTF-8 text files directly inside `folder`, sorted by name. Symbolic
/// links, subfolders, oversized or unreadable files are skipped; an unreadable folder yields none.
pub(crate) fn folder_templates(folder: &Path) -> Vec<FileTemplateEntryDto> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let source_name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let mut files: Vec<(String, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (!name.starts_with('.')).then(|| (name, entry.path()))
        })
        .collect();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
        .into_iter()
        .filter_map(|(name, path)| {
            let small =
                std::fs::metadata(&path).is_ok_and(|meta| meta.len() <= MAX_FOLDER_TEMPLATE_BYTES);
            if !small {
                return None;
            }
            let body = std::fs::read_to_string(&path).ok()?;
            Some(FileTemplateEntryDto {
                id: format!("folder:{name}"),
                label: name.clone(),
                file_name: name,
                body,
                source: FileTemplateSourceDto::Folder,
                source_name: source_name.clone(),
            })
        })
        .take(MAX_FOLDER_TEMPLATES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_templates_list_small_text_files_sorted_and_skip_the_rest() {
        let directory = tempfile::tempdir().expect("temp directory");
        std::fs::write(directory.path().join("b.md"), "# B\n").expect("write");
        std::fs::write(directory.path().join("a.txt"), "A").expect("write");
        std::fs::write(directory.path().join(".hidden"), "x").expect("write");
        std::fs::write(directory.path().join("binary.bin"), [0xff, 0xfe]).expect("write");
        std::fs::write(
            directory.path().join("big.txt"),
            vec![b'x'; MAX_FOLDER_TEMPLATE_BYTES as usize + 1],
        )
        .expect("write");
        std::fs::create_dir(directory.path().join("subdir")).expect("mkdir");

        let templates = folder_templates(directory.path());
        let names: Vec<&str> = templates.iter().map(|t| t.file_name.as_str()).collect();
        assert_eq!(names, ["a.txt", "b.md"]);
        assert_eq!(templates[0].id, "folder:a.txt");
        assert_eq!(templates[0].body, "A");
        assert!(
            templates
                .iter()
                .all(|t| t.source == FileTemplateSourceDto::Folder)
        );
    }

    #[test]
    fn a_leading_tilde_resolves_against_the_home_directory() {
        let home = crate::workspace::resolve_home_directory();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/Templates"), home.join("Templates"));
        assert_eq!(
            expand_home("/abs/dir"),
            std::path::PathBuf::from("/abs/dir")
        );
    }

    #[test]
    fn a_missing_folder_contributes_nothing() {
        let directory = tempfile::tempdir().expect("temp directory");
        assert!(folder_templates(&directory.path().join("absent")).is_empty());
    }

    #[test]
    fn catalog_orders_app_then_folder_then_plugin_and_skips_disabled_app_templates() {
        let directory = tempfile::tempdir().expect("temp directory");
        std::fs::write(directory.path().join("from-folder.txt"), "F").expect("write");
        let mut file_templates = fm_settings::default_file_templates();
        file_templates.truncate(2);
        file_templates[1].enabled = false;
        let folder = directory.path().to_string_lossy().into_owned();
        let plugin = FileTemplateEntryDto {
            id: "plugin:example.p:t".into(),
            label: "Plugin".into(),
            file_name: "p.txt".into(),
            body: String::new(),
            source: FileTemplateSourceDto::Plugin,
            source_name: Some("Example".into()),
        };

        let ids: Vec<String> = catalog(&file_templates, Some(&folder), vec![plugin])
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(
            ids,
            ["text", "folder:from-folder.txt", "plugin:example.p:t"]
        );
    }
}
