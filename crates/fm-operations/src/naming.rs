//! Name generation for duplicated entries.

/// Compression suffixes that continue a `.tar` extension, so `a.tar.gz` stays one extension.
const TAR_COMPRESSIONS: [&str; 7] = ["gz", "bz2", "xz", "zst", "lz", "lz4", "z"];

/// Splits a file name into its stem and extension (with the leading dot). Dotfiles such as
/// `.env`, names that end in a dot, and directory names have no extension.
fn split_extension(name: &str) -> (&str, &str) {
    let Some(dot) = name.rfind('.') else {
        return (name, "");
    };
    if dot == 0 || dot + 1 == name.len() {
        return (name, "");
    }
    let mut start = dot;
    let compression = &name[dot + 1..];
    if TAR_COMPRESSIONS
        .iter()
        .any(|known| compression.eq_ignore_ascii_case(known))
    {
        let stem = &name[..dot];
        if stem.len() > 4 && stem[stem.len() - 4..].eq_ignore_ascii_case(".tar") {
            start = dot - 4;
        }
    }
    name.split_at(start)
}

/// Returns the name of the `copy_index`-th duplicate of `name`: `report (1).pdf`.
///
/// The index goes before the extension, so a copy keeps the type of its original. Directories
/// (`is_directory`), dotfiles and extensionless names get the index appended: `v1.2 (1)`.
#[must_use]
pub fn duplicate_name(name: &str, is_directory: bool, copy_index: u32) -> String {
    let index = copy_index.max(1);
    let (stem, extension) = if is_directory {
        (name, "")
    } else {
        split_extension(name)
    };
    format!("{stem} ({index}){extension}")
}

#[cfg(test)]
mod tests {
    use super::duplicate_name;

    #[test]
    fn puts_the_index_before_the_extension() {
        assert_eq!(duplicate_name("report.pdf", false, 1), "report (1).pdf");
        assert_eq!(duplicate_name("report.pdf", false, 2), "report (2).pdf");
        assert_eq!(duplicate_name("a.b.txt", false, 1), "a.b (1).txt");
        assert_eq!(duplicate_name("資料.txt", false, 1), "資料 (1).txt");
    }

    #[test]
    fn keeps_compound_tar_extensions_together() {
        assert_eq!(
            duplicate_name("archive.tar.gz", false, 1),
            "archive (1).tar.gz"
        );
        assert_eq!(
            duplicate_name("Archive.TAR.XZ", false, 3),
            "Archive (3).TAR.XZ"
        );
        assert_eq!(duplicate_name("data.gz", false, 1), "data (1).gz");
        assert_eq!(duplicate_name(".tar.gz", false, 1), ".tar (1).gz");
    }

    #[test]
    fn appends_to_names_without_an_extension() {
        assert_eq!(duplicate_name(".env", false, 1), ".env (1)");
        assert_eq!(duplicate_name("Makefile", false, 1), "Makefile (1)");
        assert_eq!(duplicate_name("trailing.", false, 1), "trailing. (1)");
        assert_eq!(duplicate_name("v1.2", true, 1), "v1.2 (1)");
    }
}
