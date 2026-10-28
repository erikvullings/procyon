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
                let mut dots = 0;
                let mut spaced = false;
                while cursor < chars.len() && chars[cursor].1 == '.' {
                    cursor += 1;
                    dots += 1;
                    // `. . . .` leaders: absorb dots separated by whitespace.
                    let gap_end = chars[cursor..]
                        .iter()
                        .position(|(_, character)| !character.is_whitespace())
                        .map_or(chars.len(), |offset| cursor + offset);
                    if gap_end > cursor && gap_end < chars.len() && chars[gap_end].1 == '.' {
                        spaced = true;
                        cursor = gap_end;
                    }
                }
                if cursor > start {
                    let separated_before = start == 0 || chars[start - 1].1.is_whitespace();
                    let separated_after = cursor == chars.len() || chars[cursor].1.is_whitespace();
                    // A spaced leader may hang off the preceding word ("Introduction. . . .").
                    if dots >= 4 && (separated_before || spaced) && separated_after {
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
        || is_low_information(text)
}

/// Text with too little language to carry a topic: a stray cell (`.070`, `a a a`) or a statistics
/// table flattened to text, mostly digits, signs and brackets. Prose is overwhelmingly letters
/// even when it quotes figures.
#[must_use]
fn is_low_information(text: &str) -> bool {
    let (letters, total) = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .fold((0_usize, 0_usize), |(letters, total), character| {
            (letters + usize::from(character.is_alphabetic()), total + 1)
        });
    letters < 25 || total >= 80 && letters * 10 < total * 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattened_statistics_tables_are_back_matter_but_prose_with_figures_is_not() {
        let table = "16 −0.080 ( −0.492, 0.332) 0.002 ( −0.012, 0.016) F(1, 14) = 0.087 .773 .096 .286 \
                     F(1, 14) = 0.028 .869 .096 .288 7 11 −0.105 ( −0.598, 0.388) 3 5 −0.035 ( −0.787, 0.718)";
        assert!(is_back_matter(&[], table));
        assert!(is_back_matter(&[], ".070"));
        assert!(is_back_matter(&[], "a a a"));
        assert!(!is_back_matter(
            &[],
            "The effect was larger (d = 0.758, 95% CI 0.4 to 1.1) for the criminal history domain than for neighbourhood factors, which showed no significant effect overall."
        ));
    }

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

    #[test]
    fn spaced_dot_leaders_are_navigation() {
        let (text, navigation) = clean_pdf_toc_text("Introduction. . . . . . . . . 425");
        assert!(navigation);
        assert_eq!(text, "Introduction 425");
        let (text, navigation) = clean_pdf_toc_text("Wait. . . and then");
        assert!(!navigation);
        assert_eq!(text, "Wait. . . and then");
    }
}
