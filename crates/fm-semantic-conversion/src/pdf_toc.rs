//! Conservative PDF table-of-contents detection and leader cleanup.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
