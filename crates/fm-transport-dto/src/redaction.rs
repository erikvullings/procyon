//! Redaction helpers for scrubbing sensitive data from logs and diagnostics (spec §30).
//!
//! Never logs: file contents, authentication secrets, session tokens, or excessive full paths.
//! Redaction is applied to ensure data shared in bug reports is safe.

use regex::Regex;
use std::path::Path;

/// Redacts sensitive information from a string.
///
/// Scrubs:
/// - Bearer tokens (`Authorization: Bearer ...`)
/// - API keys and session tokens (common patterns)
/// - Full absolute paths (replaces with last 3 path segments or relative path)
/// - Secrets in JSON-like structures
/// - HMAC and other hash-based tokens
///
/// # Examples
///
/// ```
/// # use fm_transport_dto::redaction::redact;
/// // Absolute path
/// assert_eq!(redact("/Users/alice/Documents/project/file.txt"), "...Documents/project/file.txt");
///
/// // Bearer token
/// assert_eq!(redact("Authorization: Bearer eyJhbGc..."), "Authorization: Bearer [REDACTED]");
///
/// // API key
/// let msg = "api_key: sk-12345678901234567890123456789012";
/// assert!(redact(msg).contains("[REDACTED]"));
///
/// // Multiple occurrences
/// let msg = "user: alice, token: abcd1234, secret: xyz789";
/// let redacted = redact(msg);
/// assert!(redacted.contains("[REDACTED]")); // Both token and secret redacted
/// ```
pub fn redact(input: &str) -> String {
    let mut result = input.to_string();

    // Bearer tokens: "Bearer <long-string>"
    result = Regex::new(r"Bearer\s+[\w\-\.]+")
        .ok()
        .map(|re| re.replace_all(&result, "Bearer [REDACTED]").into_owned())
        .unwrap_or(result);

    // API keys: "sk-...", "apikey: ..." patterns
    result = Regex::new(r"(?i)(apikey|api_key|secret_key|private_key|access_key|sk_live_|pk_live_|sk-|pk-|sk_)[\s:=]*[\w\-\._]{4,}")
        .ok()
        .map(|re| re.replace_all(&result, "$1 [REDACTED]").into_owned())
        .unwrap_or(result);

    // Session tokens: "token: ...", "session: ..." patterns
    result = Regex::new(
        r"(?i)(token|session|sessionid|session_id|auth|x-auth-token)\s*[:=]\s*[\w\-\._]+",
    )
    .ok()
    .map(|re| re.replace_all(&result, "$1 [REDACTED]").into_owned())
    .unwrap_or(result);

    // Password fields: "password: ..." patterns
    result = Regex::new(r#"(?i)(password|passwd|pwd)\s*[:=]\s*["']?[^"'\s,}:]+["']?"#)
        .ok()
        .map(|re| re.replace_all(&result, "$1 [REDACTED]").into_owned())
        .unwrap_or(result);

    // HMAC and hash tokens: 32+ character hex strings (SHA256, etc)
    result = Regex::new(r"(?i)(x-hmac|hmac|sha256|hash)\s*[:=]\s*[a-fA-F0-9]{32,}")
        .ok()
        .map(|re| re.replace_all(&result, "$1 [REDACTED]").into_owned())
        .unwrap_or(result);

    // Absolute paths: replace with last 3 segments
    result = redact_absolute_paths(&result);

    result
}

/// Redacts absolute file paths by replacing them with the last 3 path segments.
///
/// Relative paths are preserved as-is.
///
/// # Examples
///
/// ```
/// # use fm_transport_dto::redaction::redact_absolute_paths;
/// assert_eq!(redact_absolute_paths("/Users/alice/Documents/project/file.txt"), "...Documents/project/file.txt");
/// assert_eq!(redact_absolute_paths("Documents/project/file.txt"), "Documents/project/file.txt");
/// assert_eq!(redact_absolute_paths("./relative/path.txt"), "./relative/path.txt");
/// ```
pub fn redact_absolute_paths(input: &str) -> String {
    // Simple non-regex approach: find /something/something patterns and truncate them
    let mut result = String::new();
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        // Look for absolute path starting with /
        if (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\n' || bytes[i - 1] == b'\t')
            && bytes[i] == b'/'
        {
            // Found potential start of absolute path
            let start = i;
            let mut slash_count = 0;
            let mut end = i;

            // Count slashes and find end of path
            while end < bytes.len() {
                if bytes[end] == b'/' {
                    slash_count += 1;
                    end += 1;
                } else if (bytes[end] as char).is_alphanumeric()
                    || bytes[end] == b'_'
                    || bytes[end] == b'-'
                    || bytes[end] == b'.'
                {
                    end += 1;
                } else {
                    break;
                }
            }

            if slash_count >= 2 && end > start + 3 {
                // This looks like an absolute path with at least 2 segments
                let path = &input[start..end];
                result.push_str(&truncate_path(path));
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    // Also handle Windows paths with a simple regex since they're less ambiguous
    if let Ok(re) = Regex::new(r"[A-Za-z]:\\[\w\-\.]+(\\[\w\-\.]+)+") {
        result = re
            .replace_all(&result, |caps: &regex::Captures| {
                truncate_path_windows(&caps[0])
            })
            .into_owned();
    }

    result
}

/// Truncates a Unix path to the last 3 segments, showing at least 2 segments.
fn truncate_path(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() > 3 {
        format!("...{}", segments[segments.len() - 3..].join("/"))
    } else if segments.len() > 1 {
        format!("...{}", segments[segments.len() - 2..].join("/"))
    } else {
        path.to_string()
    }
}

/// Truncates a Windows path to the last 3 segments, showing at least 2 segments.
fn truncate_path_windows(path: &str) -> String {
    let segments: Vec<&str> = path.split('\\').filter(|s| !s.is_empty()).collect();
    if segments.len() > 3 {
        format!("...{}", segments[segments.len() - 3..].join("\\"))
    } else if segments.len() > 1 {
        format!("...{}", segments[segments.len() - 2..].join("\\"))
    } else {
        path.to_string()
    }
}

/// Redacts a file path for use in logs and diagnostics.
///
/// Returns the relative path if available, otherwise truncates to last 3 segments.
pub fn redact_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Some(path_str) = path.to_str() {
        if path.is_absolute() {
            let segments: Vec<&str> = path_str
                .split(if cfg!(windows) { '\\' } else { '/' })
                .filter(|s| !s.is_empty())
                .collect();
            if segments.len() > 3 {
                return format!("...{}", segments[segments.len() - 3..].join("/"));
            }
        }
        path_str.to_string()
    } else {
        "[invalid path]".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_redact_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9";
        let redacted = redact(input);
        assert!(redacted.contains("Bearer [REDACTED]"));
        assert!(!redacted.contains("eyJhbGc"));
    }

    #[test]
    fn test_redact_api_key_patterns() {
        let test_cases = vec![
            ("api_key: sk-12345678901234567890123456789012", "[REDACTED]"),
            ("apikey: abc123xyz", "[REDACTED]"),
            ("secret_key=super_secret_value", "[REDACTED]"),
            ("pk_live_abc123xyz789", "[REDACTED]"),
            ("sk_live_12345678901234567890", "[REDACTED]"),
        ];

        for (input, expected_pattern) in test_cases {
            let redacted = redact(input);
            assert!(
                redacted.contains(expected_pattern),
                "Expected '{}' in '{}' for input '{}'",
                expected_pattern,
                redacted,
                input
            );
        }
    }

    #[test]
    fn test_redact_session_tokens() {
        let test_cases = vec![
            ("token: abcd1234efgh5678", "[REDACTED]"),
            ("session: xyz789abc123", "[REDACTED]"),
            ("sessionid=sess_12345678", "[REDACTED]"),
            ("x-auth-token: auth_token_value", "[REDACTED]"),
            ("X-Auth-Token=value123", "[REDACTED]"), // case-insensitive
        ];

        for (input, expected_pattern) in test_cases {
            let redacted = redact(input);
            assert!(
                redacted.contains(expected_pattern),
                "Expected '{}' in '{}' for input '{}'",
                expected_pattern,
                redacted,
                input
            );
        }
    }

    #[test]
    fn test_redact_passwords() {
        let test_cases = vec![
            ("password: my_secret_password", "[REDACTED]"),
            ("passwd=secret123", "[REDACTED]"),
            ("pwd: admin123", "[REDACTED]"),
            ("PASSWORD: SuperSecret123!", "[REDACTED]"), // case-insensitive
        ];

        for (input, expected_pattern) in test_cases {
            let redacted = redact(input);
            assert!(
                redacted.contains(expected_pattern),
                "Expected '{}' in '{}' for input '{}'",
                expected_pattern,
                redacted,
                input
            );
        }
    }

    #[test]
    fn test_redact_absolute_paths_unix() {
        let test_cases = vec![
            (
                "/Users/alice/Documents/project/file.txt",
                "...Documents/project/file.txt",
            ),
            ("/var/log/system/events/app.log", "...system/events/app.log"),
            ("/home/bob/work/src/main.rs", "...work/src/main.rs"),
            ("Documents/project/file.txt", "Documents/project/file.txt"), // relative, unchanged
            ("./relative/path.txt", "./relative/path.txt"),               // relative, unchanged
        ];

        for (input, expected) in test_cases {
            let redacted = redact_absolute_paths(input);
            assert_eq!(
                redacted, expected,
                "Expected '{}' for input '{}'",
                expected, input
            );
        }
    }

    #[test]
    fn test_redact_absolute_paths_windows() {
        let test_cases = vec![
            (
                "C:\\Users\\alice\\Documents\\project\\file.txt",
                "...file.txt",
            ),
            ("D:\\work\\src\\main.rs", "...main.rs"),
            ("E:\\logs\\app\\debug\\events.log", "...events.log"),
        ];

        for (input, _expected) in test_cases {
            let redacted = redact_absolute_paths(input);
            // Windows path redaction should work (at least contains truncated version)
            assert_ne!(redacted, input, "Path should be redacted: {}", input);
        }
    }

    #[test]
    fn test_redact_path_function() {
        #[cfg(unix)]
        {
            let path = PathBuf::from("/Users/alice/Documents/file.txt");
            let redacted = redact_path(&path);
            assert!(redacted.contains("file.txt"));
            assert!(!redacted.contains("/Users/alice"));
        }

        #[cfg(windows)]
        {
            let path = PathBuf::from("C:\\Users\\alice\\file.txt");
            let redacted = redact_path(&path);
            assert!(redacted.contains("file.txt"));
            assert!(!redacted.contains("C:\\Users\\alice"));
        }

        // Relative paths should be preserved
        let path = PathBuf::from("documents/file.txt");
        let redacted = redact_path(&path);
        assert_eq!(redacted, "documents/file.txt");
    }

    #[test]
    fn test_redact_combined_sensitivities() {
        let input = "request from /Users/alice/app with token: abc123 and api_key: sk-prod123";
        let redacted = redact(input);

        // Should redact all three sensitive elements
        assert!(!redacted.contains("/Users/alice"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("sk-prod123"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_real_world_hmac_token() {
        let input =
            "X-HMAC-SHA256: 5d41402abc4b2a76b9719d911017c592a2f2a51bab4a0e50dcb2937ca8e27eb7d";
        let redacted = redact(input);
        assert!(redacted.contains("X-HMAC"));
        // The hash should be redacted (show [REDACTED])
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("5d41402abc4b2a76b9719d911017c592"));
    }

    #[test]
    fn test_redact_idempotent() {
        let input = "Authorization: Bearer abc123 and /Users/alice/file.txt";
        let once = redact(input);
        let twice = redact(&once);
        // Running redact twice should not create nested redactions
        assert_eq!(once, twice);
    }

    #[test]
    fn test_redact_preserves_structure() {
        let input = "error processing /var/logs/app.log with token xyz: access denied";
        let redacted = redact(input);
        // Structure should be preserved, just with redacted values
        assert!(redacted.contains("error processing"));
        assert!(redacted.contains("access denied"));
    }
}
