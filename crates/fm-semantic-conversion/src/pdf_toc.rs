//! Conservative PDF table-of-contents cleanup and extracted DOI repair.

/// Repairs the line-layout gap between Elsevier's `j.` DOI stem and its
/// journal code. Other whitespace and ordinary prose are left untouched.
#[must_use]
pub fn repair_spaced_elsevier_doi(text: &str) -> String {
    const PREFIXES: [&str; 2] = ["https://doi.org/10.1016/j.", "http://doi.org/10.1016/j."];
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some((position, prefix)) = PREFIXES
        .iter()
        .filter_map(|prefix| remaining.find(prefix).map(|position| (position, *prefix)))
        .min_by_key(|(position, _)| *position)
    {
        let boundary = position + prefix.len();
        result.push_str(&remaining[..boundary]);
        remaining = &remaining[boundary..];
        let next = remaining.trim_start_matches(char::is_whitespace);
        let journal_len = next.bytes().take_while(u8::is_ascii_alphabetic).count();
        let suffix = next.get(journal_len..).unwrap_or_default();
        let valid_suffix = journal_len > 0
            && suffix.starts_with('.')
            && suffix
                .get(1..5)
                .is_some_and(|year| year.bytes().all(|byte| byte.is_ascii_digit()))
            && suffix.get(5..6) == Some(".");
        if next.len() < remaining.len() && valid_suffix {
            remaining = next;
        }
    }
    result.push_str(remaining);
    result
}

/// Removes dot leaders only when a run of at least four dots is separated
/// from words by whitespace (or a line boundary). Ordinary ellipses survive.
#[must_use]
pub fn clean_pdf_toc_text(text: &str) -> (String, bool) {
    let mut navigation = false;
    let lines = text
        .lines()
        .map(|line| {
            let mut cleaned = String::new();
            let mut cursor = 0;
            let chars = line.char_indices().collect::<Vec<_>>();
            while cursor < chars.len() {
                let start = cursor;
                while cursor < chars.len() && chars[cursor].1 == '.' {
                    cursor += 1;
                }
                if cursor > start {
                    let separated_before = start == 0 || chars[start - 1].1.is_whitespace();
                    let separated_after = cursor == chars.len() || chars[cursor].1.is_whitespace();
                    if cursor - start >= 4 && separated_before && separated_after {
                        navigation = true;
                        cleaned.push(' ');
                        continue;
                    }
                    for (_, character) in &chars[start..cursor] {
                        cleaned.push(*character);
                    }
                } else {
                    cleaned.push(chars[cursor].1);
                    cursor += 1;
                }
            }
            if cleaned == line {
                line.to_owned()
            } else {
                cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (lines, navigation)
}

/// Known structural headings identify navigation even when PDF extraction
/// does not retain the leaders themselves.
#[must_use]
pub fn is_pdf_toc_section(section_path: &[String]) -> bool {
    section_path.iter().any(|component| {
        matches!(
            component.trim().to_ascii_lowercase().as_str(),
            "table of contents" | "contents" | "inhoudsopgave"
        )
    })
}

/// Heading titles (lower-cased, without numbering) that open a bibliography.
const REFERENCE_HEADINGS: &[&str] = &[
    "references",
    "reference list",
    "bibliography",
    "works cited",
    "literature cited",
    "literature",
    "referenties",
    "literatuur",
    "literatuurlijst",
    "bronnen",
    "bronnenlijst",
    "bibliografie",
    "literaturverzeichnis",
    "literatur",
    "quellenverzeichnis",
    "références",
    "bibliographie",
    "referencias",
    "bibliografía",
    "referências",
    "bibliografia",
    "riferimenti",
    "piśmiennictwo",
];

/// Whether a structural path sits under a references / bibliography heading.
#[must_use]
pub fn is_references_section(section_path: &[String]) -> bool {
    section_path.iter().any(|component| {
        let title = component
            .trim()
            .trim_start_matches(|character: char| {
                character.is_ascii_digit() || matches!(character, '.' | ')' | ' ')
            })
            .trim_end_matches([':', '.'])
            .to_lowercase();
        REFERENCE_HEADINGS.contains(&title.as_str())
    })
}

/// Citations an APA-style entry list needs before text counts as a reference list.
const MIN_REFERENCE_ENTRIES: usize = 3;

/// Whether text is dominated by bibliography entries such as `Kelly, J. (1986). Title`,
/// for lists whose heading was not recognised as a heading.
#[must_use]
pub fn looks_like_reference_list(text: &str) -> bool {
    let bytes = text.as_bytes();
    let entries = bytes
        .windows(7)
        .filter(|window| {
            window[0] == b'('
                && window[1..5].iter().all(u8::is_ascii_digit)
                && matches!(window[5], b')' | b'a'..=b'z')
                && (window[5] == b')' && window[6] == b'.'
                    || window[5] != b')' && window[6] == b')')
        })
        .count();
    entries >= MIN_REFERENCE_ENTRIES
}

/// Whether a chunk is table-of-contents or reference-list material that should not shape
/// corpus-level analysis.
#[must_use]
pub fn is_back_matter(section_path: &[String], text: &str) -> bool {
    is_pdf_toc_section(section_path)
        || is_references_section(section_path)
        || clean_pdf_toc_text(text).1
        || looks_like_reference_list(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_reference_headings_and_citation_lists() {
        assert!(is_references_section(&["7. References".into()]));
        assert!(is_references_section(&[
            "Intro".into(),
            "LITERATUUR".into()
        ]));
        assert!(!is_references_section(&["Referencing methods".into()]));
        assert!(looks_like_reference_list(
            "Kelly, J. (1986). Title one. Pittman, K. (1996). Title two. Rutter, M. (1987). Three."
        ));
        assert!(!looks_like_reference_list(
            "Resilience (Rutter, 1987). Risk factors (Durlak, 1997). A later study (2004) agreed."
        ));
    }

    #[test]
    fn cleans_only_separated_navigation_leaders() {
        assert_eq!(
            clean_pdf_toc_text(
                "........................ Aggression\nAlcohol/drug abuse .......... 12\nWait... then finish....start"
            ),
            (
                "Aggression\nAlcohol/drug abuse 12\nWait... then finish....start".into(),
                true
            )
        );
        assert_eq!(
            clean_pdf_toc_text("An ordinary passage... and a sentence."),
            ("An ordinary passage... and a sentence.".into(), false)
        );
    }

    #[test]
    fn repairs_only_spaced_elsevier_doi_stems() {
        assert_eq!(
            repair_spaced_elsevier_doi(
                "https://doi.org/10.1016/j. chiabu.2011.01.002; j. chiabu.2011.01.002; https://doi.org/10.1016/j.chiabu.2018.07.034"
            ),
            "https://doi.org/10.1016/j.chiabu.2011.01.002; j. chiabu.2011.01.002; https://doi.org/10.1016/j.chiabu.2018.07.034"
        );
        assert_eq!(
            repair_spaced_elsevier_doi("https://doi.org/10.1016/j. Next sentence. 2011"),
            "https://doi.org/10.1016/j. Next sentence. 2011"
        );
    }
}
