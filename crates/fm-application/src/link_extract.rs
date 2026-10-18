//! Pure link extraction for the broken-link doctor (task 0266).
//!
//! Nothing here touches the filesystem, executes content or fetches anything: each function
//! turns text (or bytes) into the link targets it contains, and [`relative_path_target`] decides
//! which of those are local paths worth checking at all.

use percent_encoding::percent_decode_str;

/// A link found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedLink {
    /// The target exactly as written (before decoding or fragment removal).
    pub(crate) target: String,
    /// One-based line the link starts on.
    pub(crate) line: u32,
}

fn link(target: &str, line: usize) -> ExtractedLink {
    ExtractedLink {
        target: target.to_owned(),
        line: u32::try_from(line).unwrap_or(u32::MAX),
    }
}

/// Turns an extracted target into a decoded relative path to check, or `None` for anchors,
/// absolute URLs (any scheme), protocol-relative links and empty targets. A fragment or query is
/// dropped; absolute filesystem paths are kept so they can be checked as written.
pub(crate) fn relative_path_target(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    let scheme_end = trimmed.find(':');
    if let Some(index) = scheme_end {
        let scheme = &trimmed[..index];
        let looks_like_scheme = scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
        if looks_like_scheme {
            return None;
        }
    }
    let without_fragment = trimmed.split(['#', '?']).next().unwrap_or_default();
    if without_fragment.is_empty() {
        return None;
    }
    Some(
        percent_decode_str(without_fragment)
            .decode_utf8_lossy()
            .into_owned(),
    )
}

/// Blanks inline code spans so `` `[a](b)` `` is not mistaken for a link.
fn without_code_spans(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut in_code = false;
    for character in line.chars() {
        if character == '`' {
            in_code = !in_code;
            output.push(' ');
        } else if in_code {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

/// Reads an inline link destination starting right after `](`.
fn inline_destination(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('<') {
        return stripped.find('>').map(|end| stripped[..end].to_owned());
    }
    let mut depth = 0_i32;
    let mut end = rest.len();
    for (index, character) in rest.char_indices() {
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => {
                end = index;
                break;
            }
            ')' => depth -= 1,
            c if c.is_whitespace() => {
                end = index;
                break;
            }
            _ => {}
        }
    }
    let destination = &rest[..end];
    (!destination.is_empty()).then(|| destination.to_owned())
}

/// Inline links, images and reference definitions in Markdown, skipping fenced and inline code.
pub(crate) fn markdown_links(text: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let mut fence: Option<String> = None;
    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = raw_line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        if let Some(marker) = marker {
            match &fence {
                Some(open) if open == marker => fence = None,
                None => fence = Some(marker.to_owned()),
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let line = without_code_spans(raw_line);
        let mut search_from = 0;
        while let Some(found) = line[search_from..].find("](") {
            let start = search_from + found + 2;
            if let Some(destination) = inline_destination(&line[start..]) {
                links.push(link(&destination, line_number));
            }
            search_from = start;
        }
        // Reference definition: `[id]: destination "title"`.
        let definition = line.trim_start();
        if let Some(rest) = definition.strip_prefix('[')
            && let Some(close) = rest.find("]:")
            && !rest[..close].contains('[')
        {
            let destination = rest[close + 2..].trim_start();
            let destination = destination
                .strip_prefix('<')
                .and_then(|inner| inner.find('>').map(|end| &inner[..end]))
                .unwrap_or_else(|| destination.split_whitespace().next().unwrap_or_default());
            if !destination.is_empty() {
                links.push(link(destination, line_number));
            }
        }
    }
    links
}

const HTML_LINK_ATTRIBUTES: &[&str] = &["href", "src", "poster"];

/// `href`, `src` and `poster` attributes in HTML, skipping comments.
pub(crate) fn html_links(text: &str) -> Vec<ExtractedLink> {
    let mut links = Vec::new();
    let bytes = text.as_bytes();
    let mut position = 0;
    let mut line = 1_usize;
    let mut counted_to = 0;
    while position < bytes.len() {
        let Some(offset) = text[position..].find('<') else {
            break;
        };
        let tag_start = position + offset;
        if text[tag_start..].starts_with("<!--") {
            position = text[tag_start..]
                .find("-->")
                .map_or(bytes.len(), |end| tag_start + end + 3);
            continue;
        }
        let tag_end = text[tag_start..]
            .find('>')
            .map_or(bytes.len(), |end| tag_start + end);
        line += text[counted_to..tag_start].matches('\n').count();
        counted_to = tag_start;
        let tag = &text[tag_start + 1..tag_end];
        let first = tag.chars().next();
        if first.is_some_and(|c| c.is_ascii_alphabetic()) {
            links.extend(tag_attribute_links(tag, line));
        }
        position = tag_end.max(tag_start + 1);
    }
    links
}

fn tag_attribute_links(tag: &str, line: usize) -> Vec<ExtractedLink> {
    let mut found = Vec::new();
    let characters: Vec<char> = tag.chars().collect();
    let mut index = 0;
    // Skip the tag name.
    while index < characters.len() && !characters[index].is_whitespace() {
        index += 1;
    }
    while index < characters.len() {
        while index < characters.len()
            && (characters[index].is_whitespace() || characters[index] == '/')
        {
            index += 1;
        }
        let name_start = index;
        while index < characters.len()
            && !characters[index].is_whitespace()
            && !matches!(characters[index], '=' | '/' | '>')
        {
            index += 1;
        }
        let name: String = characters[name_start..index]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        if index >= characters.len() || characters[index] != '=' {
            continue;
        }
        index += 1;
        while index < characters.len() && characters[index].is_whitespace() {
            index += 1;
        }
        let value: String = match characters.get(index) {
            Some(&quote) if quote == '"' || quote == '\'' => {
                index += 1;
                let start = index;
                while index < characters.len() && characters[index] != quote {
                    index += 1;
                }
                let value = characters[start..index].iter().collect();
                index += 1;
                value
            }
            _ => {
                let start = index;
                while index < characters.len() && !characters[index].is_whitespace() {
                    index += 1;
                }
                characters[start..index].iter().collect()
            }
        };
        if HTML_LINK_ATTRIBUTES.contains(&name.as_str()) && !value.is_empty() {
            found.push(link(&value, line));
        }
    }
    found
}

/// Playlist formats the doctor understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaylistKind {
    M3u,
    Pls,
    Xspf,
}

impl PlaylistKind {
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "m3u" | "m3u8" => Some(Self::M3u),
            "pls" => Some(Self::Pls),
            "xspf" => Some(Self::Xspf),
            _ => None,
        }
    }
}

fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Track targets in a playlist file.
pub(crate) fn playlist_links(kind: PlaylistKind, text: &str) -> Vec<ExtractedLink> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match kind {
        PlaylistKind::M3u => text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let line = line.trim();
                (!line.is_empty() && !line.starts_with('#')).then(|| link(line, index + 1))
            })
            .collect(),
        PlaylistKind::Pls => text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let (key, value) = line.split_once('=')?;
                let key = key.trim().to_ascii_lowercase();
                let is_file = key.strip_prefix("file").is_some_and(|rest| {
                    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
                });
                (is_file && !value.trim().is_empty()).then(|| link(value.trim(), index + 1))
            })
            .collect(),
        PlaylistKind::Xspf => {
            let mut links = Vec::new();
            let mut search_from = 0;
            while let Some(open) = text[search_from..].find("<location>") {
                let start = search_from + open + "<location>".len();
                let Some(close) = text[start..].find("</location>") else {
                    break;
                };
                let line = text[..start].matches('\n').count() + 1;
                links.push(link(
                    unescape_xml(text[start..start + close].trim()).as_str(),
                    line,
                ));
                search_from = start + close;
            }
            links
        }
    }
}

/// The local target recorded in a Windows `.lnk` shell link, if it has one.
///
/// Follows the MS-SHLLINK layout: a 76-byte header, an optional LinkTargetIDList, then the
/// LinkInfo structure whose local base path is a NUL-terminated ANSI string. Shortcuts that only
/// carry an ID list (or are malformed) return `None`.
pub(crate) fn shortcut_target(bytes: &[u8]) -> Option<String> {
    const HEADER_SIZE: usize = 0x4C;
    if bytes.len() < HEADER_SIZE || bytes[..4] != [0x4C, 0, 0, 0] {
        return None;
    }
    let flags = u32::from_le_bytes(bytes[0x14..0x18].try_into().ok()?);
    let has_id_list = flags & 0x1 != 0;
    let has_link_info = flags & 0x2 != 0;
    if !has_link_info {
        return None;
    }
    let mut offset = HEADER_SIZE;
    if has_id_list {
        let size = usize::from(u16::from_le_bytes(
            bytes.get(offset..offset + 2)?.try_into().ok()?,
        ));
        offset += 2 + size;
    }
    let info = bytes.get(offset..)?;
    let info_size = usize::try_from(u32::from_le_bytes(info.get(0..4)?.try_into().ok()?)).ok()?;
    let info = info.get(..info_size)?;
    let info_flags = u32::from_le_bytes(info.get(8..12)?.try_into().ok()?);
    if info_flags & 0x1 == 0 {
        return None;
    }
    let base_offset =
        usize::try_from(u32::from_le_bytes(info.get(16..20)?.try_into().ok()?)).ok()?;
    let raw = info.get(base_offset..)?;
    let end = raw.iter().position(|byte| *byte == 0)?;
    let path = String::from_utf8_lossy(&raw[..end]).into_owned();
    (!path.is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(links: &[ExtractedLink]) -> Vec<&str> {
        links.iter().map(|link| link.target.as_str()).collect()
    }

    #[test]
    fn markdown_finds_inline_images_and_reference_links_with_line_numbers() {
        let text =
            "See [doc](docs/a.md \"Title\") and ![img](<img/my pic.png>).\n\n[ref]: ./b.md\nplain";
        let links = markdown_links(text);
        assert_eq!(targets(&links), ["docs/a.md", "img/my pic.png", "./b.md"]);
        assert_eq!(links.iter().map(|l| l.line).collect::<Vec<_>>(), [1, 1, 3]);
    }

    #[test]
    fn markdown_ignores_code_and_keeps_parentheses_in_targets() {
        let text = "`[x](no.md)`\n```\n[y](fenced.md)\n```\n[z](a_(1).md)\n~~~\n[w](tilde.md)\n~~~";
        assert_eq!(targets(&markdown_links(text)), ["a_(1).md"]);
    }

    #[test]
    fn html_reads_href_src_and_poster_but_not_comments() {
        let text = "<a href=\"a.html\">x</a>\n<!-- <img src=\"hidden.png\"> -->\n<img src='img/p.png' alt=x>\n<video poster=thumb.jpg src=v.mp4></video>";
        let links = html_links(text);
        assert_eq!(
            targets(&links),
            ["a.html", "img/p.png", "thumb.jpg", "v.mp4"]
        );
        assert_eq!(links[1].line, 3);
    }

    #[test]
    fn relative_path_target_filters_non_local_links_and_decodes() {
        assert_eq!(
            relative_path_target("a%20b.md#top").as_deref(),
            Some("a b.md")
        );
        assert_eq!(
            relative_path_target("../x/y.png?v=1").as_deref(),
            Some("../x/y.png")
        );
        assert_eq!(
            relative_path_target("/abs/file.txt").as_deref(),
            Some("/abs/file.txt")
        );
        for skipped in [
            "#anchor",
            "https://x.test/a",
            "mailto:a@b.c",
            "//cdn/x.js",
            "",
            "data:image/png;base64,AA==",
        ] {
            assert_eq!(relative_path_target(skipped), None, "{skipped}");
        }
        assert_eq!(
            relative_path_target("C:\\Music\\a.mp3").as_deref(),
            Some("C:\\Music\\a.mp3")
        );
    }

    #[test]
    fn playlists_yield_their_track_targets() {
        let m3u = playlist_links(PlaylistKind::M3u, "#EXTM3U\n#EXTINF:1,x\na.mp3\n\nb.mp3");
        assert_eq!(targets(&m3u), ["a.mp3", "b.mp3"]);
        let pls = playlist_links(
            PlaylistKind::Pls,
            "[playlist]\nFile1=a.mp3\nTitle1=x\nFile2=b.mp3\nNumberOfEntries=2",
        );
        assert_eq!(targets(&pls), ["a.mp3", "b.mp3"]);
        let xspf = playlist_links(
            PlaylistKind::Xspf,
            "<playlist><trackList><track><location>a%20b.mp3</location></track>\n<track><location>x&amp;y.mp3</location></track></trackList></playlist>",
        );
        assert_eq!(targets(&xspf), ["a%20b.mp3", "x&y.mp3"]);
        assert_eq!(xspf[1].line, 2);
    }

    /// Builds a minimal `.lnk` with a LinkInfo block holding `path` as its local base path.
    fn lnk(path: &str, with_id_list: bool) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x4C];
        bytes[..4].copy_from_slice(&[0x4C, 0, 0, 0]);
        let flags: u32 = if with_id_list { 0x3 } else { 0x2 };
        bytes[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        if with_id_list {
            bytes.extend_from_slice(&4_u16.to_le_bytes());
            bytes.extend_from_slice(&[1, 2, 3, 4]);
        }
        let header_size = 28_u32;
        let base_offset = header_size;
        let total = header_size as usize + path.len() + 1;
        let mut info = Vec::new();
        info.extend_from_slice(&(total as u32).to_le_bytes());
        info.extend_from_slice(&header_size.to_le_bytes());
        info.extend_from_slice(&1_u32.to_le_bytes());
        info.extend_from_slice(&0_u32.to_le_bytes());
        info.extend_from_slice(&base_offset.to_le_bytes());
        info.extend_from_slice(&[0; 8]);
        info.extend_from_slice(path.as_bytes());
        info.push(0);
        bytes.extend_from_slice(&info);
        bytes
    }

    #[test]
    fn shortcut_target_reads_the_local_base_path() {
        assert_eq!(
            shortcut_target(&lnk("C:\\Docs\\a.txt", false)).as_deref(),
            Some("C:\\Docs\\a.txt")
        );
        assert_eq!(
            shortcut_target(&lnk("D:\\x.exe", true)).as_deref(),
            Some("D:\\x.exe")
        );
    }

    #[test]
    fn malformed_shortcuts_are_rejected_without_panicking() {
        assert_eq!(shortcut_target(b"not a shortcut"), None);
        let mut truncated = lnk("C:\\a.txt", false);
        truncated.truncate(0x50);
        assert_eq!(shortcut_target(&truncated), None);
    }
}
