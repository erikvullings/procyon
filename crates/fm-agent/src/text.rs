//! Small text helpers shared by the tools and their hosts.

/// The first `max_bytes` of `text`, cut at a character boundary.
pub fn truncate_to_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The last `max_bytes` of `text`, cut at a character boundary.
pub fn newest_bytes(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_land_on_character_boundaries() {
        assert_eq!(truncate_to_bytes("héllo", 2), "h");
        assert_eq!(truncate_to_bytes("abc", 9), "abc");
        assert_eq!(newest_bytes("héllo", 4), "llo");
        assert_eq!(newest_bytes("abc", 9), "abc");
    }
}
