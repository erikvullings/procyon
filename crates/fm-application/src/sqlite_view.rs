//! Read-only SQLite database viewer sessions (task 0251).
//!
//! Untrusted database files are an attack surface, so every connection is opened
//! `SQLITE_OPEN_READ_ONLY` with `immutable=1` (no journal, WAL or `-shm` side effects on the
//! user's file), defensive mode, tight length limits and an authorizer that only admits
//! `SELECT`/read statements plus a whitelist of introspection pragmas. Each query also runs under
//! a wall-clock deadline enforced by the SQLite progress handler.
//!
//! rusqlite needs a real file, so local files are opened in place and any other provider's file
//! is first copied into a size-capped temporary file that lives as long as the session.

use crate::rejection::{Rejection, reason};
use crate::viewer_warning::{ViewerWarning, codes as viewer_codes};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    OpenSqliteViewRequestDto, OpenSqliteViewResponseDto, QuerySqliteViewRequestDto,
    QuerySqliteViewResponseDto, SqliteCellDto, SqliteColumnDto, SqliteColumnFilterDto,
    SqliteFilterOperatorDto, SqliteObjectDto, SqliteObjectKindDto, SqliteQuerySourceDto,
    SqliteSortDto, SqliteValueTypeDto, SqliteViewSessionRequestDto,
};
use fm_vfs::{EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry};
use rusqlite::config::DbConfig;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, OpenFlags};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;
use crate::structured_view::read_source_range;

const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const MAX_SESSIONS: usize = 16;
/// Largest non-local database copied into a temporary file.
const TEMP_COPY_LIMIT_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_ROWS: u16 = 100;
const MAX_ROWS_PER_REQUEST: u16 = 500;
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// SQLite VM instructions between deadline checks.
const PROGRESS_INTERVAL_OPS: i32 = 2_000;
const MAX_SQL_BYTES: usize = 64 * 1024;
/// Largest single string/blob SQLite may build while evaluating an expression.
const MAX_VALUE_BYTES: i32 = 16 * 1024 * 1024;
const MAX_SEARCH_COLUMNS: usize = 64;
const MAX_FILTERS: usize = 16;
const CELL_TEXT_CHARS: usize = 2_048;
const BLOB_PREVIEW_BYTES: usize = 32;
/// Soft cap on the display bytes of one response page.
const RESPONSE_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const READ_PRAGMAS: [&str; 18] = [
    "table_info",
    "table_xinfo",
    "table_list",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "database_list",
    "collation_list",
    "compile_options",
    "page_count",
    "page_size",
    "freelist_count",
    "encoding",
    "user_version",
    "application_id",
    "schema_version",
    "data_version",
];
/// Header fields a pragma could rewrite; denied whenever an argument is supplied.
const WRITE_CAPABLE_PRAGMAS: [&str; 5] = [
    "user_version",
    "application_id",
    "schema_version",
    "page_size",
    "encoding",
];

struct Session {
    connection: Mutex<Connection>,
    /// Browsable objects, used to validate identifiers before they reach generated SQL.
    objects: Vec<SqliteObjectDto>,
    /// Keeps a remote file's local copy alive; deleted on drop.
    _temp: Option<tempfile::TempPath>,
}

/// Shared session owner used by both HTTP and Tauri adapters.
#[derive(Clone)]
pub(crate) struct SqliteViewService {
    providers: ProviderRegistry,
    sessions: Arc<RwLock<HashMap<Uuid, Arc<Session>>>>,
    /// Opens still copying or reading their schema; they count towards [`MAX_SESSIONS`].
    pending: Arc<AtomicUsize>,
}

/// Releases an in-flight open's reservation when the open finishes or fails.
struct PendingSlot(Arc<AtomicUsize>);

impl Drop for PendingSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl SqliteViewService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) async fn open(
        &self,
        request: OpenSqliteViewRequestDto,
    ) -> Result<OpenSqliteViewResponseDto, ApplicationError> {
        let _slot = self.reserve_slot().await?;
        let location: Location = request.location.into();
        let provider = self
            .providers
            .resolve(&location)
            .map_err(ApplicationError::from)?;
        provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?
            .require(ProviderCapabilities::READ)
            .map_err(ApplicationError::from)?;
        let entry = EntryRef {
            id: EntryId::new(),
            location: location.clone(),
        };
        let cancellation = CancellationToken::new();
        let summary = provider
            .inspect(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        if summary.kind != EntryKind::File {
            return Err(ApplicationError::from(Rejection::new(
                reason::NOT_REGULAR_FILE,
                "database views require a regular file",
            )));
        }
        let source_bytes = provider
            .file_size(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        let random_access = provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?
            .contains(ProviderCapabilities::RANDOM_ACCESS);
        let head = read_source_range(
            &provider,
            &entry,
            random_access,
            0,
            SQLITE_MAGIC.len() as u64,
            cancellation.clone(),
        )
        .await?;
        if head.as_slice() != SQLITE_MAGIC {
            return Err(ApplicationError::from(Rejection::new(
                reason::NOT_SQLITE,
                "not a SQLite database",
            )));
        }
        let (path, temp) = match location.to_native_path() {
            Ok(path) => (path, None),
            Err(_) => {
                let temp = copy_to_temp(&provider, &entry, source_bytes, cancellation).await?;
                (temp.to_path_buf(), Some(temp))
            }
        };
        let warning = wal_warning(&path, temp.is_some());
        let (connection, objects, version) = tokio::task::spawn_blocking(move || {
            let connection = open_read_only(&path)?;
            let objects = read_schema(&connection)?;
            install_authorizer(&connection);
            let version = rusqlite::version().to_owned();
            Ok::<_, ApplicationError>((connection, objects, version))
        })
        .await
        .map_err(|_| ApplicationError::Internal)??;
        let session_id = Uuid::new_v4();
        let session = Arc::new(Session {
            connection: Mutex::new(connection),
            objects: objects.clone(),
            _temp: temp,
        });
        self.sessions.write().await.insert(session_id, session);
        Ok(OpenSqliteViewResponseDto {
            session_id,
            source_bytes,
            sqlite_version: version,
            objects,
            warning_code: warning.as_ref().map(|warning| warning.code.to_owned()),
            warning: warning.map(|warning| warning.text),
        })
    }

    pub(crate) async fn query(
        &self,
        request: QuerySqliteViewRequestDto,
    ) -> Result<QuerySqliteViewResponseDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        tokio::task::spawn_blocking(move || {
            let connection = session
                .connection
                .lock()
                .map_err(|_| ApplicationError::Internal)?;
            run_query(&connection, &session.objects, &request)
        })
        .await
        .map_err(|_| ApplicationError::Internal)?
    }

    pub(crate) async fn close(
        &self,
        request: SqliteViewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        self.sessions
            .write()
            .await
            .remove(&request.session_id)
            .map(|_| ())
            .ok_or(ApplicationError::NotFound)
    }

    /// Counts this open against the session cap from the start, so concurrent opens (which may
    /// each hold a large temporary copy) cannot all pass a check made before any of them finishes.
    async fn reserve_slot(&self) -> Result<PendingSlot, ApplicationError> {
        let pending = self.pending.fetch_add(1, Ordering::AcqRel) + 1;
        let slot = PendingSlot(Arc::clone(&self.pending));
        if self.sessions.read().await.len() + pending > MAX_SESSIONS {
            return Err(ApplicationError::from(
                Rejection::new(
                    reason::TOO_MANY_PREVIEWS,
                    "too many open database views; close one first",
                )
                .with("max", MAX_SESSIONS),
            ));
        }
        Ok(slot)
    }

    async fn session(&self, id: Uuid) -> Result<Arc<Session>, ApplicationError> {
        self.sessions
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or(ApplicationError::NotFound)
    }
}

/// Copies a non-local database into a temporary file, refusing anything over the cap.
async fn copy_to_temp(
    provider: &Arc<dyn FileSystemProvider>,
    entry: &EntryRef,
    source_bytes: u64,
    cancellation: CancellationToken,
) -> Result<tempfile::TempPath, ApplicationError> {
    if source_bytes > TEMP_COPY_LIMIT_BYTES {
        return Err(ApplicationError::from(Rejection::new(reason::REMOTE_DATABASE_TOO_LARGE, format!("database is larger than the {} MiB limit for remote files; copy it locally to view it", TEMP_COPY_LIMIT_BYTES / (1024 * 1024))).with("limitBytes", TEMP_COPY_LIMIT_BYTES)));
    }
    let reader = provider
        .open_read(entry, cancellation)
        .await
        .map_err(ApplicationError::from)?;
    let temp = tempfile::Builder::new()
        .prefix("fm-sqlite-")
        .suffix(".db")
        .tempfile()
        .map_err(|_| ApplicationError::Internal)?;
    let (std_file, temp_path) = temp.into_parts();
    let mut file = tokio::fs::File::from_std(std_file);
    let copied = tokio::io::copy(&mut reader.take(TEMP_COPY_LIMIT_BYTES + 1), &mut file)
        .await
        .map_err(read_stream_error)?;
    if copied > TEMP_COPY_LIMIT_BYTES {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::REMOTE_DATABASE_TOO_LARGE,
                "database exceeded the size limit for remote files",
            )
            .with("limitBytes", TEMP_COPY_LIMIT_BYTES),
        ));
    }
    Ok(temp_path)
}

/// Warns when a WAL file sits next to a local database, because `immutable` mode ignores it.
fn wal_warning(path: &Path, copied: bool) -> Option<ViewerWarning> {
    if copied {
        return None;
    }
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let has_wal = std::fs::metadata(PathBuf::from(wal)).is_ok_and(|meta| meta.len() > 0);
    has_wal.then(|| {
        ViewerWarning::new(
            viewer_codes::WAL_NOT_SHOWN,
            "A write-ahead log (-wal) exists next to this database; writes that were not yet \
             checkpointed are not shown.",
        )
    })
}

/// Builds the `file:` URI SQLite needs for `immutable=1`, percent-encoding reserved characters.
fn sqlite_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut bytes = Vec::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        match byte {
            b'%' | b'?' | b'#' | b' ' => bytes.extend_from_slice(format!("%{byte:02X}").as_bytes()),
            _ => bytes.push(byte),
        }
    }
    let encoded = String::from_utf8_lossy(&bytes).into_owned();
    if encoded.starts_with("//") {
        format!("file:{encoded}?mode=ro&immutable=1")
    } else if encoded.starts_with('/') {
        format!("file://{encoded}?mode=ro&immutable=1")
    } else {
        // Windows drive path such as `C:/dir/file.db`.
        format!("file:///{encoded}?mode=ro&immutable=1")
    }
}

fn open_read_only(path: &Path) -> Result<Connection, ApplicationError> {
    let connection = Connection::open_with_flags(
        sqlite_uri(path),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(map_sqlite_error)?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(map_sqlite_error)?;
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)
        .map_err(map_sqlite_error)?;
    connection
        .set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_VALUE_BYTES)
        .map_err(map_sqlite_error)?;
    connection
        .set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, MAX_SQL_BYTES as i32)
        .map_err(map_sqlite_error)?;
    connection
        .set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
        .map_err(map_sqlite_error)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(map_sqlite_error)?;
    Ok(connection)
}

/// Admits only read statements; everything that could write, attach or reconfigure is denied.
fn install_authorizer(connection: &Connection) {
    connection.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Function { function_name } => {
            if function_name.eq_ignore_ascii_case("load_extension") {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }
        AuthAction::Pragma {
            pragma_name,
            pragma_value,
        } => {
            let name = pragma_name.to_ascii_lowercase();
            let allowed = READ_PRAGMAS.contains(&name.as_str())
                && !(pragma_value.is_some() && WRITE_CAPABLE_PRAGMAS.contains(&name.as_str()));
            if allowed {
                Authorization::Allow
            } else {
                Authorization::Deny
            }
        }
        _ => Authorization::Deny,
    }));
}

fn read_schema(connection: &Connection) -> Result<Vec<SqliteObjectDto>, ApplicationError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY name COLLATE NOCASE",
        )
        .map_err(map_sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_error)?;
    let mut objects = Vec::with_capacity(rows.len());
    for (kind, name, create_sql) in rows {
        // A broken view or an unavailable virtual-table module must not hide the other tables:
        // such an object is listed without columns.
        let columns = read_columns(connection, &name).unwrap_or_default();
        objects.push(SqliteObjectDto {
            name,
            kind: if kind == "view" {
                SqliteObjectKindDto::View
            } else {
                SqliteObjectKindDto::Table
            },
            columns,
            create_sql,
        });
    }
    Ok(objects)
}

fn read_columns(connection: &Connection, name: &str) -> rusqlite::Result<Vec<SqliteColumnDto>> {
    let mut info = connection.prepare(&format!("PRAGMA table_info({})", quote_identifier(name)))?;
    info.query_map([], |row| {
        Ok(SqliteColumnDto {
            name: row.get(1)?,
            declared_type: row.get(2)?,
            not_null: row.get::<_, i64>(3)? != 0,
            primary_key: row.get::<_, i64>(5)? != 0,
        })
    })?
    .collect()
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Maps SQLite failures to messages that are safe and useful to show in the viewer.
fn map_sqlite_error(error: rusqlite::Error) -> ApplicationError {
    use rusqlite::ErrorCode;
    let message = match &error {
        rusqlite::Error::SqliteFailure(failure, detail) => match failure.code {
            ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt => {
                "the file is not a readable SQLite database (it may be corrupt or encrypted)"
                    .to_owned()
            }
            ErrorCode::OperationInterrupted => {
                format!(
                    "the query was stopped after {} seconds",
                    QUERY_TIMEOUT.as_secs()
                )
            }
            ErrorCode::AuthorizationForStatementDenied => {
                "only read-only statements are allowed".to_owned()
            }
            ErrorCode::TooBig => "a value is too large to evaluate".to_owned(),
            _ => detail.clone().unwrap_or_else(|| error.to_string()),
        },
        rusqlite::Error::MultipleStatement => "only one statement can be run at a time".to_owned(),
        other => other.to_string(),
    };
    ApplicationError::InvalidRequest(message)
}

/// Interrupts the running statement once [`QUERY_TIMEOUT`] has elapsed from now.
fn arm_deadline(connection: &Connection) {
    let deadline = Instant::now() + QUERY_TIMEOUT;
    connection.progress_handler(
        PROGRESS_INTERVAL_OPS,
        Some(move || Instant::now() >= deadline),
    );
}

fn run_query(
    connection: &Connection,
    objects: &[SqliteObjectDto],
    request: &QuerySqliteViewRequestDto,
) -> Result<QuerySqliteViewResponseDto, ApplicationError> {
    let started = Instant::now();
    let limit = request.limit.clamp(1, MAX_ROWS_PER_REQUEST);
    let limit = if request.limit == 0 {
        DEFAULT_ROWS
    } else {
        limit
    };
    arm_deadline(connection);
    let result = match &request.source {
        SqliteQuerySourceDto::Browse {
            object,
            search,
            filters,
            sort,
        } => browse(
            connection,
            objects,
            object,
            search,
            filters,
            sort.as_ref(),
            request.offset,
            limit,
        ),
        SqliteQuerySourceDto::Sql { sql } => run_sql(connection, sql, request.offset, limit),
    };
    connection.progress_handler(PROGRESS_INTERVAL_OPS, None::<fn() -> bool>);
    let mut response = result?;
    response.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn browse(
    connection: &Connection,
    objects: &[SqliteObjectDto],
    object: &str,
    search: &str,
    filters: &[SqliteColumnFilterDto],
    sort: Option<&SqliteSortDto>,
    offset: u64,
    limit: u16,
) -> Result<QuerySqliteViewResponseDto, ApplicationError> {
    let schema = objects
        .iter()
        .find(|candidate| candidate.name == object)
        .ok_or_else(|| {
            ApplicationError::from(
                Rejection::new(reason::UNKNOWN_TABLE, format!("unknown table '{object}'"))
                    .with("name", object),
            )
        })?;
    if filters.len() > MAX_FILTERS {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::TOO_MANY_FILTERS,
                format!("at most {MAX_FILTERS} column filters are supported"),
            )
            .with("max", MAX_FILTERS),
        ));
    }
    let mut conditions: Vec<String> = Vec::new();
    let mut parameters: Vec<Value> = Vec::new();
    let search = search.trim();
    if !search.is_empty() && !schema.columns.is_empty() {
        let pattern = like_pattern(search);
        let clauses: Vec<String> = schema
            .columns
            .iter()
            .take(MAX_SEARCH_COLUMNS)
            .map(|column| {
                parameters.push(Value::Text(pattern.clone()));
                format!(
                    "CAST({} AS TEXT) LIKE ? ESCAPE '\\'",
                    quote_identifier(&column.name)
                )
            })
            .collect();
        conditions.push(format!("({})", clauses.join(" OR ")));
    }
    for filter in filters {
        let column = schema
            .columns
            .iter()
            .find(|candidate| candidate.name == filter.column)
            .ok_or_else(|| {
                ApplicationError::from(
                    Rejection::new(
                        reason::UNKNOWN_COLUMN,
                        format!("unknown column '{}'", filter.column),
                    )
                    .with("name", &filter.column),
                )
            })?;
        let quoted = quote_identifier(&column.name);
        match filter.operator {
            // An empty substring or bound is the state of a just-added filter; applying it would
            // only hide NULL rows, so it is ignored until the user types a value.
            SqliteFilterOperatorDto::Contains
            | SqliteFilterOperatorDto::Greater
            | SqliteFilterOperatorDto::Less
                if filter.value.is_empty() => {}
            SqliteFilterOperatorDto::Contains => {
                parameters.push(Value::Text(like_pattern(&filter.value)));
                conditions.push(format!("CAST({quoted} AS TEXT) LIKE ? ESCAPE '\\'"));
            }
            // Numeric-looking input is bound as a number so untyped and view columns (which have
            // no type affinity to convert a text operand) still compare numerically; equality also
            // accepts the textual form for columns that store numbers as text.
            SqliteFilterOperatorDto::Equals | SqliteFilterOperatorDto::NotEquals => {
                parameters.push(typed_operand(&filter.value));
                parameters.push(Value::Text(filter.value.clone()));
                let matches = format!("({quoted} IS ? OR CAST({quoted} AS TEXT) IS ?)");
                conditions.push(if filter.operator == SqliteFilterOperatorDto::Equals {
                    matches
                } else {
                    format!("NOT {matches}")
                });
            }
            SqliteFilterOperatorDto::Greater => {
                parameters.push(typed_operand(&filter.value));
                conditions.push(format!("{quoted} > ?"));
            }
            SqliteFilterOperatorDto::Less => {
                parameters.push(typed_operand(&filter.value));
                conditions.push(format!("{quoted} < ?"));
            }
            SqliteFilterOperatorDto::IsNull => conditions.push(format!("{quoted} IS NULL")),
            SqliteFilterOperatorDto::IsNotNull => {
                conditions.push(format!("{quoted} IS NOT NULL"));
            }
        }
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };
    let order_clause = match sort {
        Some(sort) => {
            let column = schema
                .columns
                .iter()
                .find(|candidate| candidate.name == sort.column)
                .ok_or_else(|| {
                    ApplicationError::from(
                        Rejection::new(
                            reason::UNKNOWN_COLUMN,
                            format!("unknown column '{}'", sort.column),
                        )
                        .with("name", &sort.column),
                    )
                })?;
            format!(
                " ORDER BY {} {}",
                quote_identifier(&column.name),
                if sort.descending { "DESC" } else { "ASC" }
            )
        }
        None => String::new(),
    };
    let table = quote_identifier(&schema.name);
    let sql = format!("SELECT * FROM {table}{where_clause}{order_clause}");
    let mut response = read_page(connection, &sql, &parameters, offset, limit)?;
    if offset == 0 {
        // The count gets its own budget so a slow count cannot starve the page read.
        arm_deadline(connection);
        response.total_rows = count_rows(connection, &table, &where_clause, &parameters);
    }
    Ok(response)
}

/// Counts matching rows; an interrupted or failed count just omits the total.
fn count_rows(
    connection: &Connection,
    table: &str,
    where_clause: &str,
    parameters: &[Value],
) -> Option<u64> {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table}{where_clause}"),
            rusqlite::params_from_iter(parameters.iter()),
            |row| row.get::<_, i64>(0),
        )
        .ok()
        .and_then(|count| u64::try_from(count).ok())
}

fn run_sql(
    connection: &Connection,
    sql: &str,
    offset: u64,
    limit: u16,
) -> Result<QuerySqliteViewResponseDto, ApplicationError> {
    if sql.trim().is_empty() {
        return Err(ApplicationError::from(Rejection::new(
            reason::SQL_EMPTY,
            "enter a SQL statement",
        )));
    }
    if sql.len() > MAX_SQL_BYTES {
        return Err(ApplicationError::from(Rejection::new(
            reason::SQL_TOO_LONG,
            "the SQL statement is too long",
        )));
    }
    read_page(connection, sql, &[], offset, limit)
}

/// Prepares `sql`, skips `offset` rows and reads up to `limit` rows within the response budget.
fn read_page(
    connection: &Connection,
    sql: &str,
    parameters: &[Value],
    offset: u64,
    limit: u16,
) -> Result<QuerySqliteViewResponseDto, ApplicationError> {
    let mut statement = connection.prepare(sql).map_err(map_sqlite_error)?;
    if !statement.readonly() {
        return Err(ApplicationError::from(Rejection::new(
            reason::SQL_READ_ONLY,
            "only read-only statements are allowed",
        )));
    }
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut rows = statement
        .query(rusqlite::params_from_iter(parameters.iter()))
        .map_err(map_sqlite_error)?;
    let mut skipped = 0_u64;
    while skipped < offset {
        if rows.next().map_err(map_sqlite_error)?.is_none() {
            break;
        }
        skipped += 1;
    }
    let mut page: Vec<Vec<SqliteCellDto>> = Vec::new();
    let mut budget = 0_usize;
    let mut truncated = false;
    let mut has_more = false;
    while let Some(row) = rows.next().map_err(map_sqlite_error)? {
        if page.len() >= usize::from(limit) {
            has_more = true;
            break;
        }
        if budget >= RESPONSE_BUDGET_BYTES {
            truncated = true;
            has_more = true;
            break;
        }
        let mut cells = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            let cell = cell_from(row.get_ref(index).map_err(map_sqlite_error)?);
            budget += cell.text.as_ref().map_or(0, String::len);
            cells.push(cell);
        }
        page.push(cells);
    }
    Ok(QuerySqliteViewResponseDto {
        columns,
        rows: page,
        offset,
        has_more,
        total_rows: None,
        truncated,
        elapsed_ms: 0,
    })
}

fn cell_from(value: ValueRef<'_>) -> SqliteCellDto {
    match value {
        ValueRef::Null => SqliteCellDto {
            value_type: SqliteValueTypeDto::Null,
            text: None,
            byte_length: None,
            truncated: false,
        },
        ValueRef::Integer(number) => SqliteCellDto {
            value_type: SqliteValueTypeDto::Integer,
            text: Some(number.to_string()),
            byte_length: None,
            truncated: false,
        },
        ValueRef::Real(number) => SqliteCellDto {
            value_type: SqliteValueTypeDto::Real,
            text: Some(number.to_string()),
            byte_length: None,
            truncated: false,
        },
        ValueRef::Text(bytes) => {
            let full = String::from_utf8_lossy(bytes);
            let truncated = full.chars().count() > CELL_TEXT_CHARS;
            let text: String = if truncated {
                full.chars().take(CELL_TEXT_CHARS).collect()
            } else {
                full.into_owned()
            };
            SqliteCellDto {
                value_type: SqliteValueTypeDto::Text,
                text: Some(text),
                byte_length: truncated.then_some(bytes.len() as u64),
                truncated,
            }
        }
        ValueRef::Blob(bytes) => {
            let head = &bytes[..bytes.len().min(BLOB_PREVIEW_BYTES)];
            let hex = head
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            SqliteCellDto {
                value_type: SqliteValueTypeDto::Blob,
                text: Some(hex),
                byte_length: Some(bytes.len() as u64),
                truncated: bytes.len() > BLOB_PREVIEW_BYTES,
            }
        }
    }
}

/// `%…%` LIKE pattern for a literal substring, escaping LIKE wildcards with `\`.
/// Binds numeric-looking text as an integer or real, anything else as text.
fn typed_operand(value: &str) -> Value {
    let trimmed = value.trim();
    if let Ok(number) = trimmed.parse::<i64>() {
        Value::Integer(number)
    } else if let Ok(number) = trimmed.parse::<f64>()
        && number.is_finite()
    {
        Value::Real(number)
    } else {
        Value::Text(value.to_owned())
    }
}

fn like_pattern(value: &str) -> String {
    let mut pattern = String::with_capacity(value.len() + 2);
    pattern.push('%');
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(directory: &Path) -> PathBuf {
        let path = directory.join("sample.sqlite");
        let connection = Connection::open(&path).expect("create database");
        connection
            .execute_batch(
                "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, note TEXT, photo BLOB);
                 INSERT INTO people (name, note, photo) VALUES
                   ('Ada', 'first 100% programmer', x'00ff10'),
                   ('Bob', NULL, NULL),
                   ('Cy_d', '', x''),
                   ('Dee', 'looks like Ada', NULL);
                 CREATE VIEW named AS SELECT name FROM people WHERE note IS NOT NULL;
                 CREATE TABLE \"we\"\"ird\" (a, b);
                 INSERT INTO \"we\"\"ird\" VALUES (1, 2.5);",
            )
            .expect("populate");
        path
    }

    fn open(path: &Path) -> (Connection, Vec<SqliteObjectDto>) {
        let connection = open_read_only(path).expect("open read-only");
        let objects = read_schema(&connection).expect("schema");
        install_authorizer(&connection);
        (connection, objects)
    }

    fn browse_request(object: &str) -> QuerySqliteViewRequestDto {
        QuerySqliteViewRequestDto {
            session_id: Uuid::nil(),
            source: SqliteQuerySourceDto::Browse {
                object: object.to_owned(),
                search: String::new(),
                filters: Vec::new(),
                sort: None,
            },
            offset: 0,
            limit: 100,
        }
    }

    fn names(response: &QuerySqliteViewResponseDto) -> Vec<String> {
        let column = response
            .columns
            .iter()
            .position(|name| name == "name")
            .expect("name column");
        response
            .rows
            .iter()
            .map(|row| row[column].text.clone().unwrap_or_default())
            .collect()
    }

    #[test]
    fn lists_tables_views_and_columns() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (_connection, objects) = open(&fixture(directory.path()));
        let listed: Vec<_> = objects.iter().map(|object| object.name.as_str()).collect();
        assert_eq!(listed, ["named", "people", "we\"ird"]);
        let people = &objects[1];
        assert_eq!(people.kind, SqliteObjectKindDto::Table);
        assert!(people.columns[0].primary_key);
        assert!(people.columns[1].not_null);
        assert!(
            people
                .create_sql
                .as_deref()
                .unwrap()
                .starts_with("CREATE TABLE")
        );
        assert_eq!(objects[0].kind, SqliteObjectKindDto::View);
    }

    #[test]
    fn distinguishes_null_empty_and_blob_cells() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let response = run_query(&connection, &objects, &browse_request("people")).expect("browse");
        assert_eq!(response.total_rows, Some(4));
        assert_eq!(response.rows[1][2].value_type, SqliteValueTypeDto::Null);
        assert_eq!(response.rows[2][2].value_type, SqliteValueTypeDto::Text);
        assert_eq!(response.rows[2][2].text.as_deref(), Some(""));
        assert_eq!(response.rows[0][3].value_type, SqliteValueTypeDto::Blob);
        assert_eq!(response.rows[0][3].text.as_deref(), Some("00 ff 10"));
        assert_eq!(response.rows[0][3].byte_length, Some(3));
    }

    #[test]
    fn pages_with_has_more() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let mut request = browse_request("people");
        request.limit = 3;
        let first = run_query(&connection, &objects, &request).expect("first page");
        assert_eq!(first.rows.len(), 3);
        assert!(first.has_more);
        request.offset = 3;
        let second = run_query(&connection, &objects, &request).expect("second page");
        assert_eq!(second.rows.len(), 1);
        assert!(!second.has_more);
        assert_eq!(second.total_rows, None);
    }

    #[test]
    fn search_is_literal_and_covers_every_column() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let search = |text: &str| {
            let mut request = browse_request("people");
            request.source = SqliteQuerySourceDto::Browse {
                object: "people".to_owned(),
                search: text.to_owned(),
                filters: Vec::new(),
                sort: None,
            };
            names(&run_query(&connection, &objects, &request).expect("search"))
        };
        assert_eq!(search("ada"), ["Ada", "Dee"]);
        assert_eq!(search("100%"), ["Ada"]);
        assert_eq!(search("_"), ["Cy_d"]);
        assert!(search("zzz").is_empty());
    }

    #[test]
    fn filters_and_sort_combine() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let mut request = browse_request("people");
        request.source = SqliteQuerySourceDto::Browse {
            object: "people".to_owned(),
            search: String::new(),
            filters: vec![
                SqliteColumnFilterDto {
                    column: "note".to_owned(),
                    operator: SqliteFilterOperatorDto::IsNotNull,
                    value: String::new(),
                },
                SqliteColumnFilterDto {
                    column: "id".to_owned(),
                    operator: SqliteFilterOperatorDto::Greater,
                    value: "1".to_owned(),
                },
            ],
            sort: Some(SqliteSortDto {
                column: "name".to_owned(),
                descending: true,
            }),
        };
        let response = run_query(&connection, &objects, &request).expect("filter");
        assert_eq!(names(&response), ["Dee", "Cy_d"]);
        assert_eq!(response.total_rows, Some(2));
    }

    #[test]
    fn rejects_unknown_identifiers_instead_of_interpolating_them() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        assert!(
            run_query(
                &connection,
                &objects,
                &browse_request("people; DROP TABLE people")
            )
            .is_err()
        );
        let mut request = browse_request("people");
        request.source = SqliteQuerySourceDto::Browse {
            object: "people".to_owned(),
            search: String::new(),
            filters: Vec::new(),
            sort: Some(SqliteSortDto {
                column: "id; --".to_owned(),
                descending: false,
            }),
        };
        assert!(run_query(&connection, &objects, &request).is_err());
    }

    #[test]
    fn numeric_filters_compare_numerically_on_untyped_columns_and_ignore_empty_values() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = fixture(directory.path());
        Connection::open(&path)
            .expect("reopen")
            .execute_batch(
                "CREATE TABLE loose (v); INSERT INTO loose VALUES (5), (30), ('abc'), (NULL);
                 CREATE VIEW loose_view AS SELECT v AS n FROM loose;",
            )
            .expect("loose table");
        let (connection, objects) = open(&path);
        let filter = |object: &str, column: &str, operator, value: &str| {
            let mut request = browse_request(object);
            request.source = SqliteQuerySourceDto::Browse {
                object: object.to_owned(),
                search: String::new(),
                filters: vec![SqliteColumnFilterDto {
                    column: column.to_owned(),
                    operator,
                    value: value.to_owned(),
                }],
                sort: None,
            };
            run_query(&connection, &objects, &request)
                .expect("filter")
                .rows
                .len()
        };
        assert_eq!(
            filter("loose", "v", SqliteFilterOperatorDto::Equals, "5"),
            1
        );
        assert_eq!(
            filter("loose_view", "n", SqliteFilterOperatorDto::Equals, "5"),
            1
        );
        assert_eq!(
            filter("loose", "v", SqliteFilterOperatorDto::NotEquals, "5"),
            3
        );
        assert_eq!(
            filter("loose_view", "n", SqliteFilterOperatorDto::Less, "10"),
            1
        );
        // A just-added, still empty "contains" must not hide the NULL row.
        assert_eq!(
            filter("loose", "v", SqliteFilterOperatorDto::Contains, ""),
            4
        );
    }

    #[test]
    fn a_broken_view_does_not_prevent_listing_the_other_tables() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = fixture(directory.path());
        Connection::open(&path)
            .expect("reopen")
            .execute_batch(
                "PRAGMA writable_schema = ON;
                 CREATE VIEW dangling AS SELECT * FROM people;
                 DROP TABLE people;",
            )
            .ok();
        let (_connection, objects) = open(&path);
        assert!(objects.iter().any(|object| object.name == "dangling"));
        assert!(objects.iter().any(|object| object.name == "we\"ird"));
    }

    #[test]
    fn quotes_awkward_table_names() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let response =
            run_query(&connection, &objects, &browse_request("we\"ird")).expect("browse");
        assert_eq!(response.rows[0][1].text.as_deref(), Some("2.5"));
    }

    fn sql_request(sql: &str) -> QuerySqliteViewRequestDto {
        QuerySqliteViewRequestDto {
            session_id: Uuid::nil(),
            source: SqliteQuerySourceDto::Sql {
                sql: sql.to_owned(),
            },
            offset: 0,
            limit: 100,
        }
    }

    #[test]
    fn runs_read_only_sql_including_joins_cte_and_pragma() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let run = |sql: &str| run_query(&connection, &objects, &sql_request(sql));
        let counted = run("SELECT count(*) AS n FROM people").expect("count");
        assert_eq!(counted.columns, ["n"]);
        assert_eq!(counted.rows[0][0].text.as_deref(), Some("4"));
        assert!(run("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c LIMIT 5) SELECT x FROM c").is_ok());
        assert!(run("PRAGMA table_info(people)").is_ok());
        assert!(run("SELECT json_extract('{\"a\":1}', '$.a')").is_ok());
        assert!(run("EXPLAIN SELECT 1").is_ok());
    }

    #[test]
    fn bundled_sqlite_features_work_in_the_sql_console() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = fixture(directory.path());
        Connection::open(&path)
            .expect("reopen")
            .execute_batch(
                "CREATE VIRTUAL TABLE docs USING fts5(title, body);
                 INSERT INTO docs VALUES ('Rust', 'systems programming language'),
                                         ('Lua', 'small embeddable scripting language');",
            )
            .expect("fts5 table");
        let (connection, objects) = open(&path);
        let run = |sql: &str| run_query(&connection, &objects, &sql_request(sql)).expect(sql);
        assert_eq!(
            run("SELECT title FROM docs WHERE docs MATCH 'embeddable'").rows[0][0]
                .text
                .as_deref(),
            Some("Lua")
        );
        assert_eq!(
            run("SELECT name, row_number() OVER (ORDER BY name DESC) FROM people").rows[0][1]
                .text
                .as_deref(),
            Some("1")
        );
        assert!(!run("SELECT name FROM dbstat LIMIT 1").rows.is_empty());
        assert_eq!(
            run("SELECT json_group_array(name) FROM people").rows[0][0]
                .text
                .as_deref(),
            Some("[\"Ada\",\"Bob\",\"Cy_d\",\"Dee\"]")
        );
    }

    #[test]
    fn rejects_writes_attach_pragma_writes_and_multiple_statements() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = fixture(directory.path());
        let (connection, objects) = open(&path);
        let run = |sql: &str| run_query(&connection, &objects, &sql_request(sql));
        for statement in [
            "DELETE FROM people",
            "INSERT INTO people (name) VALUES ('x')",
            "DROP TABLE people",
            "CREATE TABLE t (a)",
            "UPDATE people SET name = 'x'",
            "ATTACH DATABASE ':memory:' AS other",
            "PRAGMA user_version = 7",
            "PRAGMA journal_mode = WAL",
            "VACUUM",
            "SELECT 1; SELECT 2",
            "SELECT load_extension('x')",
        ] {
            assert!(run(statement).is_err(), "{statement} must be rejected");
        }
        let (check, check_objects) = open(&path);
        let count = run_query(
            &check,
            &check_objects,
            &sql_request("SELECT count(*) FROM people"),
        )
        .expect("count");
        assert_eq!(count.rows[0][0].text.as_deref(), Some("4"));
    }

    #[test]
    fn viewing_leaves_the_file_byte_identical_even_in_wal_mode() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = fixture(directory.path());
        {
            let connection = Connection::open(&path).expect("reopen");
            connection
                .pragma_update(None, "journal_mode", "WAL")
                .expect("wal");
        }
        let before = std::fs::read(&path).expect("read before");
        let listing_before = std::fs::read_dir(directory.path()).unwrap().count();
        let (connection, objects) = open(&path);
        run_query(&connection, &objects, &browse_request("people")).expect("browse");
        drop(connection);
        assert_eq!(before, std::fs::read(&path).expect("read after"));
        assert_eq!(
            listing_before,
            std::fs::read_dir(directory.path()).unwrap().count(),
            "no -wal/-shm files may appear"
        );
    }

    #[test]
    fn stops_runaway_queries_at_the_deadline() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let started = Instant::now();
        let error = run_query(
            &connection,
            &objects,
            &sql_request(
                "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c) SELECT count(*) FROM c",
            ),
        )
        .expect_err("must be interrupted");
        assert!(started.elapsed() < QUERY_TIMEOUT + Duration::from_secs(3));
        assert!(error.to_string().contains("stopped"), "{error}");
    }

    #[test]
    fn refuses_values_over_the_length_limit() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        assert!(
            run_query(
                &connection,
                &objects,
                &sql_request("SELECT zeroblob(1000000000)")
            )
            .is_err()
        );
    }

    #[test]
    fn truncates_long_text_and_caps_page_size() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (connection, objects) = open(&fixture(directory.path()));
        let mut request = sql_request("SELECT replace(hex(zeroblob(5000)), '0', 'é')");
        request.limit = 9999;
        let response = run_query(&connection, &objects, &request).expect("long text");
        let cell = &response.rows[0][0];
        assert!(cell.truncated);
        assert_eq!(cell.text.as_ref().unwrap().chars().count(), CELL_TEXT_CHARS);
        assert!(cell.byte_length.unwrap() > CELL_TEXT_CHARS as u64);
    }

    #[test]
    fn encodes_reserved_characters_in_the_uri() {
        assert_eq!(
            sqlite_uri(Path::new("/tmp/a b#c?d%e.db")),
            "file:///tmp/a%20b%23c%3Fd%25e.db?mode=ro&immutable=1"
        );
        assert_eq!(
            sqlite_uri(Path::new("C:\\data\\x.db")),
            "file:///C:/data/x.db?mode=ro&immutable=1"
        );
    }

    #[test]
    fn opens_paths_with_reserved_characters() {
        let directory = tempfile::tempdir().expect("temp dir");
        let nested = directory.path().join(if cfg!(windows) {
            "we ird#dir%"
        } else {
            "we ird#dir?"
        });
        std::fs::create_dir(&nested).expect("nested dir");
        let (connection, objects) = open(&fixture(&nested));
        assert!(!objects.is_empty());
        drop(connection);
    }

    #[test]
    fn corrupt_files_fail_with_a_friendly_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("bad.sqlite");
        let mut bytes = SQLITE_MAGIC.to_vec();
        bytes.extend(std::iter::repeat_n(0xAB_u8, 4096));
        std::fs::write(&path, bytes).expect("write corrupt file");
        let result = open_read_only(&path).and_then(|connection| read_schema(&connection));
        let error = result.expect_err("corrupt database must fail");
        assert!(error.to_string().contains("SQLite"), "{error}");
    }
}
