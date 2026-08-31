/// Generates the deterministic same-directory duplicate name for `copy_index`.
///
/// The first copy uses `" copy"`; later copies append a number. Everything
/// from the first non-leading dot is treated as the extension, preserving
/// compound extensions such as `.tar.gz`. A lone dotfile has no extension.
#[must_use]
pub fn duplicate_name(name: &str, copy_index: u32) -> String {
    let extension_start = name
        .char_indices()
        .find_map(|(index, character)| (character == '.' && index > 0).then_some(index));
    let (stem, extension) = extension_start.map_or((name, ""), |index| name.split_at(index));
    let suffix = if copy_index <= 1 {
        " copy".to_owned()
    } else {
        format!(" copy {copy_index}")
    };
    format!("{stem}{suffix}{extension}")
}

#[cfg(test)]
mod tests {
    use super::duplicate_name;

    #[test]
    fn preserves_plain_compound_dotfile_and_unicode_names() {
        assert_eq!(duplicate_name("report.pdf", 1), "report copy.pdf");
        assert_eq!(duplicate_name("report.pdf", 2), "report copy 2.pdf");
        assert_eq!(duplicate_name("archive.tar.gz", 1), "archive copy.tar.gz");
        assert_eq!(duplicate_name(".env", 1), ".env copy");
        assert_eq!(duplicate_name("資料.txt", 1), "資料 copy.txt");
    }
}
