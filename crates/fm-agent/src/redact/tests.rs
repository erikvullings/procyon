use super::redact_secrets;

#[test]
fn a_secret_named_assignment_loses_its_value() {
    assert_eq!(
        redact_secrets("API_KEY=abc123 next"),
        "API_KEY=[redacted] next"
    );
    assert_eq!(
        redact_secrets("export GITHUB_TOKEN=\"ghp_x y\" ok"),
        "export GITHUB_TOKEN=[redacted] ok"
    );
}

#[test]
fn a_header_or_json_pair_loses_the_rest_of_the_line() {
    assert_eq!(
        redact_secrets("Authorization: Bearer abc.def"),
        "Authorization: [redacted]"
    );
    assert_eq!(
        redact_secrets("{\"token\": \"abc\"}"),
        "{\"token\": [redacted]"
    );
}

#[test]
fn a_secret_flag_loses_its_value() {
    assert_eq!(
        redact_secrets("mysql --password hunter2 -u root"),
        "mysql --password [redacted] -u root"
    );
    assert_eq!(
        redact_secrets("mysql --password=hunter2 -u root"),
        "mysql --password=[redacted] -u root"
    );
}

#[test]
fn well_known_token_shapes_are_removed_anywhere() {
    let key = format!("sk-{}", "a".repeat(32));
    let pat = format!("ghp_{}", "B".repeat(36));
    let aws = format!("AKIA{}", "Q".repeat(16));
    let text = format!("a {key} b {pat} c {aws} d");
    assert_eq!(
        redact_secrets(&text),
        "a [redacted] b [redacted] c [redacted] d"
    );
}

#[test]
fn a_private_key_block_is_replaced_whole() {
    let text = "before\n-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\nBBBB\n-----END OPENSSH PRIVATE KEY-----\nafter\n";
    assert_eq!(
        redact_secrets(text),
        "before\n[redacted private key]\nafter\n"
    );
}

#[test]
fn ordinary_output_is_unchanged() {
    let text = "drwxr-xr-x  5 me staff 160 Nov 20 src\nerror[E0382]: borrow of moved value\n";
    assert_eq!(redact_secrets(text), text);
    assert_eq!(redact_secrets("a token is needed"), "a token is needed");
}

#[test]
fn url_credentials_and_more_token_shapes_are_removed() {
    assert_eq!(
        redact_secrets("git clone https://me:hunter2@example.com/repo.git"),
        "git clone https://[redacted]@example.com/repo.git"
    );
    assert_eq!(
        redact_secrets("see https://example.com/a@b"),
        "see https://example.com/a@b"
    );
    let gitlab = format!("glpat-{}", "x".repeat(20));
    assert_eq!(redact_secrets(&format!("t {gitlab}")), "t [redacted]");
}
