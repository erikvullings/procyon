/// Generates the deterministic same-directory duplicate name for `copy_index`.
///
/// The copy index is appended to the complete original name as `" (n)"`.
#[must_use]
pub fn duplicate_name(name: &str, copy_index: u32) -> String {
    format!("{name} ({})", copy_index.max(1))
}

#[cfg(test)]
mod tests {
    use super::duplicate_name;

    #[test]
    fn preserves_plain_compound_dotfile_and_unicode_names() {
        assert_eq!(duplicate_name("report.pdf", 1), "report.pdf (1)");
        assert_eq!(duplicate_name("report.pdf", 2), "report.pdf (2)");
        assert_eq!(duplicate_name("archive.tar.gz", 1), "archive.tar.gz (1)");
        assert_eq!(duplicate_name(".env", 1), ".env (1)");
        assert_eq!(duplicate_name("資料.txt", 1), "資料.txt (1)");
    }
}
