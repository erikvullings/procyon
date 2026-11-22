//! Names that must never be read by a tool: files and folders that usually hold secrets.

/// Names that usually hold secrets; never read.
pub fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    const EXACT: &[&str] = &[
        ".env",
        ".netrc",
        ".npmrc",
        ".pgpass",
        ".pypirc",
        ".htpasswd",
        ".git-credentials",
    ];
    const PREFIXES: &[&str] = &[".env.", "id_rsa", "id_dsa", "id_ed25519", "id_ecdsa"];
    const SUFFIXES: &[&str] = &[
        ".pem",
        ".key",
        ".p12",
        ".pfx",
        ".p8",
        ".keystore",
        ".jks",
        ".tfstate",
    ];
    EXACT.contains(&lower.as_str())
        || PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
        || SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

/// Folders that hold credentials; nothing under them is read or searched.
pub fn is_credential_folder(name: &str) -> bool {
    matches!(
        name,
        ".ssh" | ".aws" | ".kube" | ".gnupg" | ".docker" | ".azure"
    )
}
