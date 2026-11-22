//! Front matter of `SKILL.md` files and notes. Only top-level keys are read, which is all the
//! skill catalogue needs (`name`, `description`), but their values may be written the ways YAML
//! allows: plain, quoted, folded across lines, or as block scalars (`|` and `>`). Nested maps and
//! lists are skipped, so a `metadata:` block cannot disturb the keys around it.

/// The top-level `key: value` pairs of a front matter block, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrontMatter {
    fields: Vec<(String, String)>,
}

impl FrontMatter {
    /// The non-empty value of `key` (case-insensitive).
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, value)| name.eq_ignore_ascii_case(key) && !value.is_empty())
            .map(|(_, value)| value.as_str())
    }
}

/// Splits a file into its front matter and its body. Without front matter the whole text is the
/// body.
pub(crate) fn split_front_matter(text: &str) -> (Option<FrontMatter>, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(after_open) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" || trimmed == "..." {
            let body = &after_open[offset + line.len()..];
            return (Some(parse(&after_open[..offset])), body);
        }
        offset += line.len();
    }
    (None, text)
}

fn parse(front: &str) -> FrontMatter {
    let lines: Vec<&str> = front
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let mut fields = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.trim().is_empty() || line.starts_with(['#', ' ', '\t']) {
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches(['"', '\'']).to_owned();
        let rest = rest.trim();
        // Everything indented (or a `- ` item at column 0) after the key belongs to its value.
        let start = index;
        while index < lines.len() {
            let next = lines[index];
            let belongs = next.trim().is_empty()
                || next.starts_with([' ', '\t'])
                || (rest.is_empty() && next.starts_with("- "));
            if !belongs {
                break;
            }
            index += 1;
        }
        let continuation = &lines[start..index];
        fields.push((key, value_of(rest, continuation)));
    }
    FrontMatter { fields }
}

fn value_of(rest: &str, continuation: &[&str]) -> String {
    if rest.starts_with(['|', '>']) {
        return block_scalar(rest, continuation);
    }
    if rest.is_empty() {
        // A nested map or a list.
        return String::new();
    }
    let folded = || {
        std::iter::once(rest)
            .chain(continuation.iter().map(|line| line.trim()))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    if let Some(quoted) = rest.strip_prefix('"') {
        let joined = folded();
        return double_quoted(joined.strip_prefix('"').unwrap_or(quoted));
    }
    if rest.starts_with('\'') {
        let joined = folded();
        let inner = joined.strip_prefix('\'').unwrap_or(&joined);
        return single_quoted(inner);
    }
    if rest.starts_with(['[', '{']) {
        return folded();
    }
    let joined = folded();
    match joined.find(" #") {
        Some(at) => joined[..at].trim_end().to_owned(),
        None => joined,
    }
}

/// A `|` or `>` scalar: the indented lines, de-indented. `|` keeps line breaks; `>` joins lines
/// with spaces and keeps a blank line as a break. Trailing blank lines are dropped.
fn block_scalar(header: &str, continuation: &[&str]) -> String {
    let folded = header.starts_with('>');
    let indent = continuation
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let lines: Vec<&str> = continuation
        .iter()
        .map(|line| line.get(indent..).unwrap_or("").trim_end())
        .collect();
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |at| at + 1);
    let lines = &lines[..end];
    if !folded {
        return lines.join("\n");
    }
    let mut text = String::new();
    let mut pending_break = false;
    for line in lines {
        if line.is_empty() {
            pending_break = true;
            continue;
        }
        if !text.is_empty() {
            text.push(if pending_break { '\n' } else { ' ' });
        }
        pending_break = false;
        text.push_str(line);
    }
    text
}

/// The text of a double-quoted scalar up to its closing quote, escapes resolved.
fn double_quoted(inner: &str) -> String {
    let mut out = String::new();
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => break,
            '\\' => match characters.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            },
            other => out.push(other),
        }
    }
    out
}

/// The text of a single-quoted scalar up to its closing quote; `''` is a quote.
fn single_quoted(inner: &str) -> String {
    let mut out = String::new();
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\'' {
            if characters.peek() == Some(&'\'') {
                characters.next();
                out.push('\'');
                continue;
            }
            break;
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests;
