//! The note file format (task 0337): optional YAML-style front matter, then Markdown. Only the
//! small subset Procyon writes is read (`key: value`, `key: [a, b]` and `- item` lists), so
//! notes written by Obsidian or by hand open too; unknown keys are kept when a note is rewritten.

/// What the front matter of a note says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NoteMeta {
    pub(crate) title: Option<String>,
    /// The skill name (task 0339); notes are virtual skills.
    pub(crate) name: Option<String>,
    /// When to use the note, as a skill description: `Use when ...`.
    pub(crate) description: Option<String>,
    pub(crate) created: Option<String>,
    pub(crate) updated: Option<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) hosts: Vec<String>,
    pub(crate) shell: Option<String>,
    pub(crate) source: Option<String>,
}

/// Longest slug, in bytes.
const MAX_SLUG_BYTES: usize = 60;

/// Splits a note into its front matter and its body. Without front matter the whole text is
/// the body.
pub(crate) fn split_note(text: &str) -> (Option<NoteMeta>, &str) {
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
            let front = &after_open[..offset];
            let body = &after_open[offset + line.len()..];
            return (Some(parse_front_matter(front)), body);
        }
        offset += line.len();
    }
    (None, text)
}

fn parse_front_matter(front: &str) -> NoteMeta {
    let mut meta = NoteMeta::default();
    let mut list_key: Option<String> = None;
    for raw in front.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some(item) = line.trim_start().strip_prefix("- ")
            && line.starts_with([' ', '\t', '-'])
        {
            if let Some(key) = &list_key {
                push_list_value(&mut meta, key, unquote(item.trim()));
            }
            continue;
        }
        list_key = None;
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() {
            list_key = Some(key);
            continue;
        }
        if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            for item in split_inline_list(inner) {
                push_list_value(&mut meta, &key, item);
            }
            continue;
        }
        let value = unquote(value);
        match key.as_str() {
            "title" => meta.title = Some(value),
            "name" => meta.name = Some(value),
            "description" => meta.description = Some(value),
            "created" => meta.created = Some(value),
            "updated" => meta.updated = Some(value),
            "shell" => meta.shell = Some(value),
            "source" => meta.source = Some(value),
            "tags" | "hosts" => push_list_value(&mut meta, &key, value),
            _ => {}
        }
    }
    meta
}

fn push_list_value(meta: &mut NoteMeta, key: &str, value: String) {
    if value.is_empty() {
        return;
    }
    match key {
        "tags" => meta.tags.push(value),
        "hosts" => meta.hosts.push(value),
        _ => {}
    }
}

/// Items of `a, "b, c", 'd'`, quotes removed; commas inside quotes stay.
fn split_inline_list(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in inner.chars() {
        match (quote, character) {
            (Some(open), c) if c == open => {
                quote = None;
                current.push(c);
            }
            (None, '"' | '\'') => {
                quote = Some(character);
                current.push(character);
            }
            (None, ',') => {
                items.push(unquote(current.trim()));
                current.clear();
            }
            _ => current.push(character),
        }
    }
    items.push(unquote(current.trim()));
    items.retain(|item| !item.is_empty());
    items
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return inner.replace("''", "'");
        }
    }
    value.to_owned()
}

/// Writes front matter and body as one note.
pub(crate) fn render_note(meta: &NoteMeta, body: &str) -> String {
    let mut out = String::from("---\n");
    let scalars = [
        ("title", &meta.title),
        ("name", &meta.name),
        ("description", &meta.description),
        ("created", &meta.created),
        ("updated", &meta.updated),
    ];
    for (key, value) in scalars {
        if let Some(value) = value {
            out.push_str(&format!("{key}: {}\n", quote(value)));
        }
    }
    for (key, values) in [("tags", &meta.tags), ("hosts", &meta.hosts)] {
        if !values.is_empty() {
            let items: Vec<String> = values.iter().map(|value| quote(value)).collect();
            out.push_str(&format!("{key}: [{}]\n", items.join(", ")));
        }
    }
    for (key, value) in [("shell", &meta.shell), ("source", &meta.source)] {
        if let Some(value) = value {
            out.push_str(&format!("{key}: {}\n", quote(value)));
        }
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

/// `value`, double-quoted when plain YAML would read it differently.
fn quote(value: &str) -> String {
    let needs = value.is_empty()
        || value.contains([':', '#', ',', '[', ']', '{', '}', '"', '\'', '\n'])
        || value.starts_with([' ', '-', '?', '!', '&', '*', '|', '>', '%', '@', '`'])
        || value.ends_with(' ');
    if needs {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', " ")
        )
    } else {
        value.to_owned()
    }
}

/// The note's title: the front-matter title, else its first `# ` heading, else the file stem.
pub(crate) fn note_title(meta: Option<&NoteMeta>, body: &str, file_stem: &str) -> String {
    if let Some(title) = meta
        .and_then(|meta| meta.title.as_deref())
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return title.to_owned();
    }
    body.lines()
        .find_map(|line| {
            line.strip_prefix("# ")
                .map(str::trim)
                .filter(|heading| !heading.is_empty())
        })
        .map_or_else(|| file_stem.to_owned(), str::to_owned)
}

/// A file name stem for a title: lowercase ASCII letters and digits joined by hyphens, at most
/// 60 characters, `note` when nothing is left.
pub(crate) fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut pending_hyphen = false;
    for character in title.chars() {
        let folded = fold(character);
        for c in folded.chars() {
            if c.is_ascii_alphanumeric() {
                if pending_hyphen && !slug.is_empty() {
                    slug.push('-');
                }
                pending_hyphen = false;
                slug.push(c.to_ascii_lowercase());
            } else {
                pending_hyphen = true;
            }
        }
    }
    if slug.len() > MAX_SLUG_BYTES {
        slug.truncate(MAX_SLUG_BYTES);
        while slug.ends_with('-') {
            slug.pop();
        }
    }
    if slug.is_empty() {
        "note".to_owned()
    } else {
        slug
    }
}

/// Common Latin letters with diacritics as plain ASCII; anything else unchanged.
fn fold(character: char) -> std::borrow::Cow<'static, str> {
    let plain = match character {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "a",
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => "i",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => "u",
        'ç' | 'Ç' | 'ć' | 'č' | 'Ć' | 'Č' => "c",
        'ñ' | 'Ñ' | 'ń' | 'Ń' => "n",
        'ł' | 'Ł' => "l",
        'ś' | 'š' | 'Ś' | 'Š' => "s",
        'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => "z",
        'ý' | 'ÿ' | 'Ý' => "y",
        'ß' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        _ => return std::borrow::Cow::Owned(character.to_string()),
    };
    std::borrow::Cow::Borrowed(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_scalars_inline_lists_and_block_lists_are_read() {
        let text = "---\ntitle: Replace nginx TLS certificate\ncreated: 2026-11-20T14:03:00Z\n\
                    tags: [nginx, tls, \"certificate\"]\nhosts:\n  - web-01\n  - 'web-02'\n\
                    shell: bash\nsource: terminal-session\naliases: [x]\n---\n# Heading\nBody\n";
        let (meta, body) = split_note(text);
        let meta = meta.expect("front matter");
        assert_eq!(meta.title.as_deref(), Some("Replace nginx TLS certificate"));
        assert_eq!(meta.created.as_deref(), Some("2026-11-20T14:03:00Z"));
        assert_eq!(meta.tags, ["nginx", "tls", "certificate"]);
        assert_eq!(meta.hosts, ["web-01", "web-02"]);
        assert_eq!(meta.shell.as_deref(), Some("bash"));
        assert_eq!(meta.source.as_deref(), Some("terminal-session"));
        assert_eq!(body, "# Heading\nBody\n");
    }

    #[test]
    fn a_note_without_front_matter_is_all_body() {
        let (meta, body) = split_note("# Just markdown\n---\nnot front matter\n");
        assert_eq!(meta, None);
        assert_eq!(body, "# Just markdown\n---\nnot front matter\n");
        // An opening fence without a closing one is body too.
        let (meta, body) = split_note("---\ntitle: x\nno end");
        assert_eq!(meta, None);
        assert_eq!(body, "---\ntitle: x\nno end");
    }

    #[test]
    fn crlf_notes_are_read() {
        let (meta, body) = split_note("---\r\ntitle: Windows\r\n---\r\nBody\r\n");
        assert_eq!(meta.unwrap().title.as_deref(), Some("Windows"));
        assert_eq!(body, "Body\r\n");
    }

    #[test]
    fn rendering_round_trips_and_quotes_values_that_need_it() {
        let meta = NoteMeta {
            title: Some("CSR: create with openssl #1".into()),
            name: Some("csr-openssl".into()),
            description: Some("Use when a CSR is needed".into()),
            created: Some("2026-11-20T14:03:00Z".into()),
            updated: Some("2026-11-20T14:03:00Z".into()),
            tags: vec!["openssl".into(), "tls, pki".into()],
            hosts: vec![],
            shell: Some("zsh".into()),
            source: Some("terminal-session".into()),
        };
        let text = render_note(&meta, "# CSR\n\nSteps\n");
        assert!(text.starts_with("---\ntitle: \"CSR: create with openssl #1\"\n"));
        assert!(text.contains("tags: [openssl, \"tls, pki\"]\n"));
        assert!(!text.contains("hosts"));
        let (parsed, body) = split_note(&text);
        assert_eq!(parsed.unwrap(), meta);
        assert_eq!(body, "# CSR\n\nSteps\n");
    }

    #[test]
    fn the_title_comes_from_front_matter_then_heading_then_file_name() {
        let meta = NoteMeta {
            title: Some("From meta".into()),
            ..NoteMeta::default()
        };
        assert_eq!(
            note_title(Some(&meta), "# From heading", "stem"),
            "From meta"
        );
        assert_eq!(
            note_title(
                Some(&NoteMeta::default()),
                "text\n#  From heading \n",
                "stem"
            ),
            "From heading"
        );
        assert_eq!(
            note_title(None, "## Not a title\ntext", "my-note"),
            "my-note"
        );
    }

    #[test]
    fn slugs_are_short_lowercase_ascii() {
        assert_eq!(
            slugify("Replace nginx TLS certificate"),
            "replace-nginx-tls-certificate"
        );
        assert_eq!(
            slugify("  CSR: create (openssl) -- v2!  "),
            "csr-create-openssl-v2"
        );
        assert_eq!(slugify("Größe über Ärger, café"), "grosse-uber-arger-cafe");
        assert_eq!(slugify("!!!"), "note");
        let long = slugify(&"word ".repeat(40));
        assert!(long.len() <= 60);
        assert!(!long.ends_with('-'));
    }
}
