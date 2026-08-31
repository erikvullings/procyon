# Security Model for Server Mode

This document describes the security architecture for the file manager's server mode (browser-based deployment), as implemented in task 0064 and beyond.

## Overview

The server mode exposes the file manager's capabilities over HTTP/REST and Server-Sent Events (SSE). Because the server controls filesystem access and runs on a network-accessible machine, it must be hardened against:

- **Unauthorized access** — session hijacking, replay attacks
- **Path traversal** — symlink escape, `..` traversal, UNC paths
- **Resource exhaustion** — oversized requests, rate limit abuse
- **Information leakage** — CORS policy bypass, error message disclosure

## Security Architecture

### 1. Session-Based Authentication (Task 0064)

All `/api/v1` routes except `GET /api/v1/health` and the Swagger/OpenAPI
surface (`/api/v1/docs`, `/api/v1/openapi.json`) require a valid session
token, enforced by the `require_session` Axum middleware
(`fm-server/src/auth.rs`) layered around every route in `build_router`
(`fm-server/src/lib.rs`). This includes the SSE stream at `GET
/api/v1/events`.

#### Token Lifecycle

- **Generation**: On server startup, a random 32-byte session secret is generated using cryptographically random UUIDs.
- **Issuance**: `fm-server` prints one token to stdout at startup (unless dev mode is active), the way tools like Jupyter print an access URL. An operator copies it into the browser client.
- **Verification**: Each request validates the token's HMAC-SHA256 signature against the server's secret, from either an `Authorization: Bearer <token>` header or a `?token=` query parameter — the latter exists because browser `EventSource` connections can't set custom headers, so the SSE stream is opened as `GET /api/v1/events?token=<token>`.
- **Lifetime**: Tokens are valid for the entire server session; no expiration or refresh tokens.

#### Token Format

```
<SHA256(secret || nonce)>-<nonce>
```

where `||` denotes concatenation and `nonce` is a unique UUID per token.

#### Development Mode

In development mode (opt-in via `--dev-mode-auth-disabled`), authentication is disabled:

- All `/api/v1` routes accept requests without a token.
- Logging warns that dev mode is active.
- **This flag is impossible to use when binding to non-loopback addresses** (enforced at startup).

#### Production Recommendations

1. Copy the token `fm-server` prints at startup, or generate one out-of-band via a secure setup process.
2. Transmit tokens over HTTPS only (the built-in `--tls-cert`/`--tls-key` termination or a reverse proxy).
3. Store tokens in secure browser storage (e.g., `sessionStorage`, never `localStorage`).
4. Restart the server to rotate the secret (requires client re-authentication); there is no separate rotation endpoint.

### 2. Loopback-Only Binding (Task 0064)

By default, the server binds to `127.0.0.1:8787` and is unreachable from the network.

#### Network Access

- **Loopback mode** (default): Server is reachable only on `localhost`.
- **LAN/WAN mode**: Requires explicit `--bind` flag + warning at startup.
  ```bash
  # WARNING: binding to non-loopback address; ensure TLS and authentication are configured
  fm-server --bind 0.0.0.0 --port 8787 --dev-mode-auth-disabled
  ```

#### Production Recommendations

1. Place the server behind a reverse proxy (nginx, Caddy) with TLS termination.
2. Use the reverse proxy to enforce authentication (e.g., HTTP Basic Auth, OAuth).
3. Never bind the fm-server directly to `0.0.0.0` in production; use a private network or VPN.

### 3. Strict CORS Policy (Task 0064)

The server implements strict origin validation with no wildcard support.

#### Configuration

```rust
// Only these origins can make cross-origin requests
--cors-origin https://example.com --cors-origin http://localhost:3000

// Empty by default (no cross-origin requests allowed)
```

#### Rationale

Wildcard CORS (`*`) allows any website to access the server's API, defeating authentication if cookies or default credentials are in use. Named origins prevent this attack.

#### Production Recommendations

1. Use a reverse proxy to serve the frontend and API from the same origin (same-origin policy).
2. If cross-origin is necessary, list only known, trusted origins.
3. Never use wildcard CORS with authentication.

### 4. Accessible Roots Validation (Task 0064)

Every incoming request that carries a filesystem `Location` is validated
against the configured accessible roots, **after symlink resolution**,
before the request reaches `FileManagerService`. This is enforced directly
in the route handlers (`crate::error::require_within_roots`, called from
`routes/directory.rs`, `routes/files.rs`, `routes/operation.rs` for every
source/destination, and `routes/search.rs` for search roots), not deep in
the filesystem provider — a rejected `Location` never reaches application
logic. Non-local providers (`archive`, `sftp`, `ftp`, `search`) are exempt:
they don't resolve to a native path on this machine.

#### Configuration

```bash
# Restrict server to only these directories
fm-server --root /home/user/documents --root /mnt/shared/public
```

#### Validation Logic

1. Resolve the requested path (follow symlinks, normalize `..` and `.`). A
   path that doesn't exist yet (e.g. about to be created) is validated by
   canonicalizing its nearest existing ancestor and rejoining the missing
   suffix, so a not-yet-created path can't be used to smuggle an escape.
2. Check if the canonical path starts with one of the configured roots.
3. Reject with `403 Forbidden` if outside all roots.

#### Escape Prevention

This blocks:

- **Path traversal**: `/home/user/documents/../../../etc/passwd` → canonicalization resolves to `/etc/passwd`, rejected.
- **Symlink escape**: `/home/user/documents/link_to_outside` → symlink resolves outside root, rejected.
- **UNC paths** (Windows): `\\?\C:\windows\system32` → canonicalization handles it.
- **Encoded traversal**: `%2e%2e` → filesystem never has this component; it's a URL encoding that doesn't affect filesystem paths.

#### Production Recommendations

1. Always configure at least one root; an empty list allows access to the entire filesystem.
2. Roots should be user-owned directories, not system directories.
3. Use read-only roots where possible (if the server is read-only).

#### Known Gap

Workspace commands (`POST /api/v1/workspaces/{id}/commands`, e.g.
`addTab`/`navigateTab`) carry `Location` values in their history/tab state
but are not validated against accessible roots at this handler, since a
workspace command's location only ever reaches the filesystem through a
subsequent `directories/list` or `navigation/open` call, which *is*
validated. Tightening this handler directly is a documented follow-up
rather than a silent gap.

### 5. Request Size Limits (Task 0064)

The server enforces a maximum request body size (default: 10 MB, configurable).

```rust
pub max_body_bytes: usize = 10 * 1024 * 1024
```

#### Rationale

Prevents denial-of-service via large payloads (e.g., uploading a 1 TB file to exhaust memory).

#### Production Recommendations

1. Set limits based on your use case:
   - Read-only server: 1 MB (for query strings only).
   - File upload support: Size of largest expected upload.
2. Pair with reverse proxy limits (nginx: `client_max_body_size`).

### 5b. Rate Limiting (Task 0064)

Mutating requests (`POST`/`PUT`/`PATCH`/`DELETE`) share one server-wide
token-bucket limiter (`fm-server/src/rate_limit.rs`, backed by the
`governor` crate); `GET`/`HEAD` requests are never throttled. Exceeding the
quota returns `429 Too Many Requests`.

```bash
# Allow at most 20 mutating requests per second, server-wide (the default)
fm-server --max-mutations-per-second 20
```

#### Rationale

Bounds how fast a single misbehaving client (or script) can issue
destructive operations, independent of the request-size limit above, which
only bounds the size of any one request.

#### Production Recommendations

1. Lower the quota for internet-facing deployments; the default of 20/s
   is tuned for a single local operator, not a shared server.
2. The limiter is server-wide, not per-client; a reverse proxy in front of
   a multi-user deployment should add its own per-IP limiting.

### 6. Audit Logging (Task 0064)

Destructive operations (delete, trash, overwrite) are logged with structured
metadata from the route handlers that trigger them:
`POST /api/v1/operations` for `delete`/`trash`/overwrite-on-conflict,
`POST /api/v1/operations/{id}/resolve-conflict` for a conflict resolved as
overwrite, and `POST /api/v1/files/editable/save` for an editable-file save
(`routes/operation.rs`, `routes/files.rs`, backed by `AuditEvent` in
`audit.rs`).

```log
audit: destructive operation
  operation=delete
  path=file:///home/user/documents/file.txt
  session_id=3f2a91
  timestamp=2024-08-10T12:00:00Z
```

#### What Is Logged

- Operation type (delete, trash, overwrite)
- The location's URI as supplied by the client
- A 6-byte SHA-256 fingerprint of the caller's session token (`session_id`) — never the token itself, so the audit log can correlate events from the same session without becoming a credential store. Absent when dev mode is active.
- Timestamp

#### What Is NOT Logged

- File contents
- Secrets (keys, tokens, passwords) — the session token is hashed to a short fingerprint before logging, never logged in full

#### Production Recommendations

1. Ship logs to a centralized logging service (e.g., Datadog, ELK).
2. Retain logs for compliance (e.g., 90 days for GDPR).
3. Use log aggregation to detect abuse patterns (e.g., many deletes by one session).

### 7. TLS/HTTPS (Task 0064)

`fm-server` can terminate TLS directly (via `axum-server`'s `rustls`
backend) or sit behind a reverse proxy; either is supported.

#### Direct TLS Termination

```bash
fm-server --bind 0.0.0.0 --tls-cert /etc/fm-server/cert.pem --tls-key /etc/fm-server/key.pem
```

Both `--tls-cert` and `--tls-key` (PEM format) must be set together; setting
only one panics at startup rather than silently serving plaintext.

#### Reverse Proxy Setup Example

```nginx
# /etc/nginx/conf.d/fm-server.conf
upstream fm_server {
    server 127.0.0.1:8787;
}

server {
    listen 443 ssl;
    server_name files.example.com;
    ssl_certificate /etc/letsencrypt/live/files.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/files.example.com/privkey.pem;

    location / {
        proxy_pass http://fm_server;
        proxy_set_header Authorization $http_authorization;
        proxy_pass_header Authorization;
    }
}
```

#### Production Recommendations

1. Use a certificate from a trusted CA (Let's Encrypt, Digicert, etc.).
2. Enable HSTS (`Strict-Transport-Security`) at the reverse proxy if used.
3. Rotate certificates before expiry; `fm-server` reads the PEM files once at startup and must be restarted to pick up a renewed certificate.

### 8. Server-Mode Configuration File (Task 0064)

Server-mode settings live in their own TOML file (`fm-server/src/config.rs`,
`ServerFileConfig`), entirely separate from the desktop app's settings
directory (`ServerConfig::settings_directory`, which stores workspace/UI
settings, not server configuration).

```bash
fm-server --config /etc/fm-server/fm-server.toml
```

```toml
# /etc/fm-server/fm-server.toml
bind = "0.0.0.0"
port = 8787
corsOrigins = ["https://files.example.com"]
roots = ["/home/user/documents", "/mnt/shared/public"]
maxBodyBytes = 10485760
maxMutationsPerSecond = 20
devModeAuthDisabled = false
tlsCert = "/etc/fm-server/cert.pem"
tlsKey = "/etc/fm-server/key.pem"
```

Precedence, highest to lowest: CLI flag or environment variable, then the
config file, then the built-in default. Every field is optional — a file
only needs to set what it wants to override.

## Threat Model

### Attacker Capabilities

- **Network**: Can eavesdrop, intercept, or replay HTTP requests.
- **Client**: Can influence browser behavior (XSS if frontend is compromised).
- **Server**: Cannot execute arbitrary code (Rust + safe memory model).

### In Scope (Mitigated)

- ✅ Unauthorized file access (authentication, accessible roots)
- ✅ Path traversal (symlink resolution, canonicalization)
- ✅ Session hijacking (HMAC-SHA256 signatures)
- ✅ Denial-of-service via large requests (request size limits)
- ✅ Cross-origin attacks (CORS policy)
- ✅ Audit trail (destructive operation logging)

### Out of Scope

- ❌ Network eavesdropping (mitigated by TLS, not in-server)
- ❌ Compromised reverse proxy (mitigated by deployment, not in-server)
- ❌ Malicious browser extensions (mitigated by CSP/SOP, not in-server)
- ❌ Client-side vulnerabilities in the frontend (frontend security, not this crate)

## Deployment Checklist

- [ ] **Bind address**: Confirm loopback binding (127.0.0.1) or reverse proxy is in place.
- [ ] **Authentication**: `--dev-mode-auth-disabled` is unset; the printed startup token is distributed to operators over a secure channel.
- [ ] **CORS origins**: Configured to known, trusted domains only (no wildcard).
- [ ] **Accessible roots**: Configured to user-owned directories only (`--root`).
- [ ] **TLS**: Either `--tls-cert`/`--tls-key` are set, or a reverse proxy terminates TLS in front of the server.
- [ ] **Request limits**: `--max-body-bytes` and `--max-mutations-per-second` set appropriately for your use case.
- [ ] **Audit logs**: Shipped to a centralized logging service.
- [ ] **Monitoring**: Alerting on auth failures, path traversal attempts (`403`/`401` rates).

## References

- **OWASP Top 10**: https://owasp.org/www-project-top-ten/
- **Node.js Security Best Practices**: https://nodejs.org/en/docs/guides/security/
- **Rust Security Considerations**: https://doc.rust-lang.org/book/ch19-01-unsafe-rust.html
- **NIST Cybersecurity Framework**: https://www.nist.gov/cyberframework

## Implementation References

- Session authentication middleware: `fm-server/src/auth.rs`, wired in `fm-server/src/lib.rs::build_router_with_service_and_session`
- Accessible roots validation: `fm-server/src/accessible_roots.rs`, called from `fm-server/src/error.rs::require_within_roots` at each route handler that accepts a `Location`
- Rate limiting: `fm-server/src/rate_limit.rs`
- Audit logging: `fm-server/src/audit.rs`, called from `fm-server/src/routes/operation.rs` and `fm-server/src/routes/files.rs`
- Server configuration and config file: `fm-server/src/config.rs` (`ServerConfig`, `ServerFileConfig`)
- CLI wiring (config file, TLS, startup token): `fm-server/src/main.rs`
- Security tests: `fm-server/tests/security.rs` (`security_tests` for pure logic, `http_security_tests` for real end-to-end HTTP coverage)
