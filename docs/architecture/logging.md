# Structured Logging and Diagnostics

Status: **In Development** (task 0073)

## Overview

This document describes the structured logging and diagnostics infrastructure for the file manager application (spec §30). The system provides:

1. **Structured tracing** with request correlation, operation tracking, and timing information
2. **Data redaction** to ensure sensitive information is never logged
3. **Diagnostics endpoint** for troubleshooting and bug report generation
4. **Error buffering** for recent non-sensitive errors

## Structured Tracing

### Tracing Spans

The backend uses `tracing` crate to instrument every HTTP request and internal operation with structured information:

```rust
// Automatically attached to every request
#[tracing::info_span]
http_request {
    request_id = "uuid",      // Unique request identifier
    method = "GET",           // HTTP method
    uri = "/api/v1/files",    // Request path
    // Duration and status tracked by TraceLayer
}
```

### Span Fields

Each operation can include additional context:

- **request_id**: UUID assigned by `MakeRequestUuid` layer
- **operation_id**: ID if the request starts a background operation
- **workspace_id**: Current workspace (if applicable)
- **plugin_id**: If executing a plugin action
- **provider_id**: VFS provider (local, sftp, etc.)
- **duration**: Wall-clock time (milliseconds)
- **result**: Success/failure status

### Configuration

Set the log level via environment variable:

```bash
RUST_LOG=info,fm_server=debug,notify::poll=error cargo run -p fm-server
```

Common levels:
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - General information (default)
- `debug` - Detailed debug output
- `trace` - Very verbose tracing

### Output Format

Logs are printed to stdout with the `tracing_subscriber::fmt()` layer:

```
2026-08-10T12:34:56.123Z INFO http_request{method="GET" uri="/api/v1/runtime" request_id="550e8400-e29b-41d4-a716-446655440000"}
```

## Data Redaction

### Policy

Logging **never includes**:
- File contents or full file paths (redacted to last 3 segments)
- Authentication secrets or session tokens (replaced with `[REDACTED]`)
- API keys or credentials (replaced with `[REDACTED]`)
- Excessive full paths in telemetry output (redacted or hashed)

### Implementation

Use the `fm_transport_dto::redaction` module to redact sensitive data:

```rust
use fm_transport_dto::redaction::{redact, redact_path};

let message = "Failed to process /Users/alice/sensitive/file.txt with token abc123";
let redacted = redact(message);
// Output: "Failed to process ...sensitive/file.txt with token [REDACTED]"

let path = redact_path("/var/log/system/app.log");
// Output: "...system/app.log"
```

### Redacted Patterns

- **Bearer tokens**: `Authorization: Bearer <token>` → `Authorization: Bearer [REDACTED]`
- **API keys**: `api_key: sk-prod123` → `api_key: [REDACTED]`
- **Session tokens**: `session: abc123def` → `session: [REDACTED]`
- **Passwords**: `password: secret123` → `password: [REDACTED]`
- **HMAC tokens**: `X-HMAC-SHA256: abc123...` → `X-HMAC-SHA256: [REDACTED]`
- **Absolute paths**: `/Users/alice/Documents/file.txt` → `...Documents/file.txt`

## Diagnostics Endpoint

### GET /api/v1/diagnostics

Returns comprehensive diagnostics information suitable for bug reports. The response is redacted and safe to share.

**Response (200 OK):**

```json
{
  "frontendVersion": "0.1.0",
  "backendVersion": "0.1.0",
  "tauriVersion": "2.0.0",
  "platform": "macOS",
  "runtimeCapabilities": { ... },
  "connectionState": {
    "connected": true,
    "lastEventReceived": "2026-08-10T12:34:56Z",
    "uptimeSeconds": 3600,
    "eventsReceived": 42,
    "statusMessage": "Connected"
  },
  "loadedPlugins": [
    {
      "pluginId": "plugin-1",
      "name": "Test Plugin",
      "enabled": true,
      "version": "1.0.0",
      "errorCount": 0
    }
  ],
  "recentErrors": [
    {
      "timestamp": "2026-08-10T12:34:50Z",
      "message": "Failed to process ...file.txt",
      "code": "FILE_ERROR",
      "context": "op-123"
    }
  ],
  "operationQueueStatus": {
    "queuedCount": 1,
    "runningCount": 1,
    "pausedCount": 0,
    "completedCount": 42,
    "totalPendingSize": 1048576
  }
}
```

### Fields

- **Version Info**: Frontend, backend, and Tauri versions for compatibility checking
- **Platform**: Operating system (macOS, Windows, Linux)
- **Runtime Capabilities**: Feature availability (native menus, plugins, etc.)
- **Connection State**: SSE/event channel status and uptime
- **Loaded Plugins**: Plugin list with enable/disable status and error counts
- **Recent Errors**: Last 50 errors in memory (bounded buffer)
- **Operation Queue**: Status of queued, running, and completed operations

## Frontend Diagnostics View

The diagnostics view (`frontend/src/features/diagnostics/`) provides a user-friendly interface:

- **Location**: Accessible via the Help or Developer menu
- **Features**:
  - Display version info and platform detection
  - Show runtime capabilities
  - Display connection status and uptime
  - List loaded plugins with status
  - Show recent errors (redacted)
  - Display operation queue metrics
  - **Copy for Bug Report** button: exports redacted diagnostics to clipboard

### Usage

```typescript
import { DiagnosticsViewComponent } from "@/features/diagnostics";

// Create component
const diagnosticsView = DiagnosticsViewComponent(fileManagerClient);

// Render in layout
m(diagnosticsView)
```

## Error Buffering

### Recent Errors

The application maintains a bounded in-memory buffer of recent non-sensitive errors:

- **Capacity**: Last 50 errors (configurable)
- **Retention**: Until app restart
- **Content**: Timestamp, error message (redacted), error code, optional context

### Error Codes

Common error codes for diagnostics:

- `INVALID_PATH` - Path validation failed
- `PERMISSION_DENIED` - Access denied
- `FILE_NOT_FOUND` - File not found
- `OPERATION_TIMEOUT` - Operation exceeded time limit
- `PLUGIN_ERROR` - Plugin execution failed
- `ARCHIVE_ERROR` - Archive operation failed

## Logging in Tests

Test code should use `tracing_subscriber::fmt().with_test_writer().try_init()` to capture logs:

```rust
#[tokio::test]
async fn test_with_logging() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .try_init();

    tracing::info!("Test log message");
}
```

## Performance Considerations

1. **Structured logging has minimal overhead** (microseconds per span)
2. **Redaction is performed only during logging** (not in regular code paths)
3. **Error buffer is bounded** to prevent memory leaks (max 1 MB total)
4. **Redaction regex patterns are compiled once** at module load

## Desktop Mode (Tauri)

### Rolling File Log

When running in desktop mode, logs can be written to a rolling file:

```bash
RUST_LOG=info fm-server 2>&1 | tee logs/fm-server-$(date +%Y%m%d-%H%M%S).log
```

### Environment Setup

Add to `.env` or launch script:

```bash
export RUST_LOG=info,fm_server=debug
export FM_LOG_DIR=$HOME/.file-manager/logs
```

## Browser Mode (Server)

### Log Output

Logs go to stdout. Redirect to files as needed:

```bash
cargo run -p fm-server 2>&1 | tee server.log &
```

### Reverse Proxy Setup

If using a reverse proxy (nginx, etc.), ensure:

1. Request IDs are forwarded in `X-Request-ID` header
2. The proxy doesn't strip sensitive headers for analysis
3. Access logs redact paths and session tokens

## Future Enhancements

- [ ] Structured error tracking service (e.g., Sentry integration)
- [ ] Historical performance data collection
- [ ] Pluggable log exporters (JSON, OpenTelemetry, etc.)
- [ ] Machine-readable logs for analysis tools
- [ ] Configurable error retention policies
- [ ] Error context capture (stack traces, local variables in debug builds)

## References

- **Spec**: file-manager-coding-agent-spec.md §30
- **Task**: TASKS/0073-diagnostics-view-and-structured-logging.md
- **Tracing Crate**: https://docs.rs/tracing/latest/tracing/
- **Related Task 0036**: Structured logging patterns (future)
