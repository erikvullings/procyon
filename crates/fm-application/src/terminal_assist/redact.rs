//! Removes likely secrets from text before it is sent to a model (task 0336).

const MARK: &str = "[redacted]";

/// Words that suggest the value after them is a secret.
const SECRET_NAMES: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "credential",
    "private_key",
];

/// Replaces private-key blocks, `name=value` and `name: value` pairs whose name suggests a secret,
/// secret-looking flags and well-known token shapes with `[redacted]`.
pub(crate) fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_key = false;
    for raw in text.split_inclusive('\n') {
        let (line, newline) = match raw.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (raw, ""),
        };
        if in_key {
            if line.contains("-----END") {
                in_key = false;
            }
            continue;
        }
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY-----") {
            in_key = !line.contains("-----END");
            out.push_str("[redacted private key]");
            out.push_str(newline);
            continue;
        }
        out.push_str(&redact_line(line));
        out.push_str(newline);
    }
    out
}

fn redact_line(line: &str) -> String {
    redact_flags(&redact_pairs(&redact_token_shapes(&redact_url_userinfo(
        line,
    ))))
}

/// `scheme://user:password@host` loses the credentials.
fn redact_url_userinfo(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(found) = rest.find("://") {
        let (head, tail) = rest.split_at(found + 3);
        out.push_str(head);
        let authority_end = tail
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace())
            .unwrap_or(tail.len());
        match tail[..authority_end].rfind('@') {
            Some(at) if tail[..at].contains(':') => {
                out.push_str(MARK);
                rest = &tail[at..];
            }
            _ => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

/// Words that are secrets by their shape alone.
fn redact_token_shapes(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut word_start: Option<usize> = None;
    let is_word = |character: char| character.is_ascii_alphanumeric() || "_-.".contains(character);
    for (index, character) in line.char_indices() {
        if is_word(character) {
            word_start.get_or_insert(index);
            continue;
        }
        if let Some(start) = word_start.take() {
            push_word(&mut out, &line[start..index]);
        }
        out.push(character);
    }
    if let Some(start) = word_start {
        push_word(&mut out, &line[start..]);
    }
    out
}

fn push_word(out: &mut String, word: &str) {
    out.push_str(if looks_like_token(word) { MARK } else { word });
}

fn looks_like_token(word: &str) -> bool {
    let long_tail = |prefix: &str, minimum: usize| {
        word.strip_prefix(prefix).is_some_and(|rest| {
            rest.len() >= minimum
                && rest
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "_-".contains(character))
        })
    };
    long_tail("sk-", 20)
        || long_tail("ghp_", 30)
        || long_tail("gho_", 30)
        || long_tail("github_pat_", 30)
        || long_tail("xoxb-", 10)
        || long_tail("xoxp-", 10)
        || long_tail("glpat-", 16)
        || long_tail("npm_", 30)
        || long_tail("sk_live_", 16)
        || long_tail("AIza", 30)
        || word.strip_prefix("AKIA").is_some_and(|rest| {
            rest.len() == 16
                && rest
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        })
}

/// `name=value` and `name: value` where the name contains a secret word.
fn redact_pairs(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    while cursor < line.len() {
        let Some((found, name)) = SECRET_NAMES
            .iter()
            .filter_map(|name| {
                lower[cursor..]
                    .find(name)
                    .map(|offset| (cursor + offset, *name))
            })
            .min_by_key(|(position, _)| *position)
        else {
            break;
        };
        // Skip the rest of the identifier, an optional closing quote and spaces.
        let mut end = found + name.len();
        let bytes = line.as_bytes();
        while end < line.len()
            && (bytes[end].is_ascii_alphanumeric() || b"_-".contains(&bytes[end]))
        {
            end += 1;
        }
        if end < line.len() && bytes[end] == b'"' {
            end += 1;
        }
        let mut probe = end;
        while probe < line.len() && bytes[probe] == b' ' {
            probe += 1;
        }
        let separator = bytes.get(probe).copied();
        if separator != Some(b'=') && separator != Some(b':') {
            out.push_str(&line[cursor..end]);
            cursor = end;
            continue;
        }
        out.push_str(&line[cursor..=probe]);
        let rest_start = probe + 1;
        let rest = &line[rest_start..];
        let value_start = rest_start + (rest.len() - rest.trim_start().len());
        out.push_str(&line[rest_start..value_start]);
        out.push_str(MARK);
        cursor = if separator == Some(b'=') {
            value_end(line, value_start)
        } else {
            line.len()
        };
    }
    out.push_str(&line[cursor..]);
    out
}

/// The end of a value that follows `=`: the closing quote, or the next space.
fn value_end(line: &str, start: usize) -> usize {
    let value = &line[start..];
    match value.chars().next() {
        Some(quote @ ('"' | '\'')) => value[1..]
            .find(quote)
            .map_or(line.len(), |close| start + 1 + close + 1),
        _ => value
            .find(char::is_whitespace)
            .map_or(line.len(), |offset| start + offset),
    }
}

/// `--password value` (no `=`): the next word is the secret.
fn redact_flags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut redact_next = false;
    for (index, part) in line.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        if redact_next && !part.is_empty() {
            out.push_str(MARK);
            redact_next = false;
            continue;
        }
        out.push_str(part);
        let lower = part.to_ascii_lowercase();
        if lower.starts_with("--")
            && !lower.contains('=')
            && SECRET_NAMES.iter().any(|name| lower.contains(name))
        {
            redact_next = true;
        }
    }
    out
}

#[cfg(test)]
mod tests;
