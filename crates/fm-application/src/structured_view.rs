//! Provider-neutral, read-only structured-data viewer sessions (task 0100).
//!
//! Sessions keep only a bounded sample, sparse record/lexer checkpoints and a
//! small hot-row cache. They never retain the source text or one offset per
//! logical record.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use calamine::{Data, Reader, open_workbook_auto_from_rs};
use csv_async::AsyncReaderBuilder;
use fm_domain::{EntryId, EntryKind, Location};
use fm_transport_dto::{
    OpenStructuredViewRequestDto, OpenStructuredViewResponseDto,
    ReadStructuredJsonWindowRequestDto, ReadStructuredJsonWindowResponseDto,
    ReadStructuredRowsRequestDto, ReadStructuredRowsResponseDto, SearchStructuredRowsRequestDto,
    SearchStructuredRowsResponseDto, StructuredCellDto, StructuredCellValueTypeDto,
    StructuredHeaderModeDto, StructuredRowDto, StructuredSheetDto, StructuredViewFormatDto,
    StructuredViewKindDto, StructuredViewSessionRequestDto, StructuredViewStatusDto,
    UpdateStructuredViewRequestDto,
};
use fm_vfs::{
    EntryRef, FileSystemProvider, ProviderCapabilities, ProviderReadStream, ProviderRegistry,
};
use futures::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::file_editor::read_stream_error;

const SAMPLE_BYTES: u64 = 64 * 1024;
const INITIAL_ROWS: usize = 200;
const MAX_ROWS_PER_REQUEST: u16 = 500;
const SPARSE_ROW_STRIDE: u64 = 1_024;
const JSON_CHECKPOINT_STRIDE: u64 = 1024 * 1024;
const HOT_PAGE_LIMIT: usize = 4;
/// Spreadsheet budgets bound both parser input and retained application state.
pub(crate) const WORKBOOK_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const WORKBOOK_EXPANDED_BYTES: u64 = 64 * 1024 * 1024;
const WORKBOOK_ENTRY_LIMIT: usize = 4_096;
const WORKBOOK_SHEET_LIMIT: usize = 64;
const WORKBOOK_ROWS_PER_SHEET: u64 = 100_000;
const WORKBOOK_COLUMNS_PER_SHEET: u32 = 2_048;
const WORKBOOK_DENSE_CELL_LIMIT: u64 = 500_000;
const WORKBOOK_NON_EMPTY_CELL_LIMIT: usize = 400_000;
const WORKBOOK_STRING_BYTES: usize = 64 * 1024;
const WORKBOOK_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const WORKBOOK_TOTAL_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct CsvCheckpoint {
    row: u64,
    byte: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct JsonLexerState {
    in_string: bool,
    escaped: bool,
    depth: u32,
    line: u64,
}

impl JsonLexerState {
    fn advance(&mut self, byte: u8) {
        if !self.in_string {
            match byte {
                b'"' => self.in_string = true,
                b'{' | b'[' => self.depth = self.depth.saturating_add(1),
                b'}' | b']' => self.depth = self.depth.saturating_sub(1),
                b'\n' => self.line = self.line.saturating_add(1),
                _ => {}
            }
            return;
        }
        if self.escaped {
            self.escaped = false;
        } else if byte == b'\\' {
            self.escaped = true;
        } else if byte == b'"' {
            self.in_string = false;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct JsonCheckpoint {
    byte: u64,
    state: JsonLexerState,
}

#[derive(Debug, Default)]
struct Progress {
    indexed_bytes: u64,
    indexed_rows: u64,
    total_rows: Option<u64>,
    complete: bool,
    warning: Option<String>,
    checkpoints: Vec<CsvCheckpoint>,
    json_checkpoints: Vec<JsonCheckpoint>,
}

#[derive(Debug, Clone)]
struct HotPage {
    start: u64,
    rows: Vec<StructuredRowDto>,
}

#[derive(Debug)]
struct SpreadsheetSheet {
    metadata: StructuredSheetDto,
    rows: BTreeMap<u64, StructuredRowDto>,
}

#[derive(Debug)]
struct SpreadsheetWorkbook {
    sheets: Vec<SpreadsheetSheet>,
    selected: usize,
}

struct Session {
    provider: Arc<dyn FileSystemProvider>,
    entry: EntryRef,
    revision: String,
    source_bytes: u64,
    format: StructuredViewFormatDto,
    kind: StructuredViewKindDto,
    random_access: bool,
    delimiter: RwLock<Option<u8>>,
    header_mode: RwLock<StructuredHeaderModeDto>,
    has_header: RwLock<bool>,
    headers: RwLock<Vec<String>>,
    sample_rows: RwLock<Vec<StructuredRowDto>>,
    workbook: Option<RwLock<SpreadsheetWorkbook>>,
    progress: RwLock<Progress>,
    hot_pages: Mutex<VecDeque<HotPage>>,
    cancellation: CancellationToken,
    configuration_generation: AtomicU64,
}

/// Shared session owner used by both HTTP and Tauri adapters.
#[derive(Clone)]
pub(crate) struct StructuredViewService {
    providers: ProviderRegistry,
    sessions: Arc<RwLock<HashMap<Uuid, Arc<Session>>>>,
}

impl StructuredViewService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn open(
        &self,
        request: OpenStructuredViewRequestDto,
    ) -> Result<OpenStructuredViewResponseDto, ApplicationError> {
        let location: Location = request.location.clone().into();
        let provider = self
            .providers
            .resolve(&location)
            .map_err(ApplicationError::from)?;
        let capabilities = provider
            .capabilities_for(&location)
            .map_err(ApplicationError::from)?;
        capabilities
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
            return Err(ApplicationError::InvalidRequest(
                "structured viewer sessions require a regular file".to_owned(),
            ));
        }
        let source_bytes = provider
            .file_size(&entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        let revision = source_revision(&summary, source_bytes);
        let random_access = capabilities.contains(ProviderCapabilities::RANDOM_ACCESS);
        let (kind, format_warning, workbook) = match request.format {
            StructuredViewFormatDto::Csv
            | StructuredViewFormatDto::Tsv
            | StructuredViewFormatDto::Ndjson => (StructuredViewKindDto::Table, None, None),
            StructuredViewFormatDto::Json => (StructuredViewKindDto::JsonText, None, None),
            StructuredViewFormatDto::Excel => {
                match load_spreadsheet(
                    &provider,
                    &entry,
                    random_access,
                    source_bytes,
                    cancellation.child_token(),
                )
                .await
                {
                    Ok(workbook) => (StructuredViewKindDto::Table, None, Some(workbook)),
                    Err(limit) => (
                        StructuredViewKindDto::ExternalFallback,
                        Some(format!(
                            "{limit} Open it in an external spreadsheet application."
                        )),
                        None,
                    ),
                }
            }
        };
        let sample = if request.format == StructuredViewFormatDto::Excel {
            Vec::new()
        } else {
            read_source_range(
                &provider,
                &entry,
                random_access,
                0,
                source_bytes.min(SAMPLE_BYTES),
                cancellation.clone(),
            )
            .await?
        };
        let delimiter = delimiter_for(&request, &sample)?;
        let (headers, rows, has_header, sheets, selected_sheet) = match request.format {
            StructuredViewFormatDto::Csv | StructuredViewFormatDto::Tsv => {
                let (headers, rows, has_header) = parse_initial_delimited_rows(
                    &sample,
                    delimiter.expect("CSV delimiter"),
                    request.header_mode,
                )
                .await?;
                (headers, rows, has_header, Vec::new(), None)
            }
            StructuredViewFormatDto::Ndjson => {
                let (headers, rows) = parse_initial_ndjson_rows(&sample);
                (headers, rows, true, Vec::new(), None)
            }
            StructuredViewFormatDto::Json => (Vec::new(), Vec::new(), false, Vec::new(), None),
            StructuredViewFormatDto::Excel => {
                if let Some(workbook) = workbook.as_ref() {
                    let selected = &workbook.sheets[workbook.selected];
                    (
                        spreadsheet_headers(selected.metadata.column_count),
                        spreadsheet_page(selected, 0, INITIAL_ROWS as u16),
                        false,
                        workbook
                            .sheets
                            .iter()
                            .map(|sheet| sheet.metadata.clone())
                            .collect(),
                        Some(selected.metadata.name.clone()),
                    )
                } else {
                    (Vec::new(), Vec::new(), false, Vec::new(), None)
                }
            }
        };
        let session_id = Uuid::new_v4();
        let indexed_rows = workbook.as_ref().map_or_else(
            || u64::try_from(rows.len()).unwrap_or(u64::MAX),
            |workbook| workbook.sheets[workbook.selected].metadata.row_count,
        );
        let session = Arc::new(Session {
            provider,
            entry,
            revision: revision.clone(),
            source_bytes,
            format: request.format,
            kind,
            random_access,
            delimiter: RwLock::new(delimiter),
            header_mode: RwLock::new(request.header_mode),
            has_header: RwLock::new(has_header),
            headers: RwLock::new(headers.clone()),
            sample_rows: RwLock::new(rows.clone()),
            workbook: workbook.map(RwLock::new),
            progress: RwLock::new(Progress {
                indexed_bytes: if kind == StructuredViewKindDto::Table
                    && request.format == StructuredViewFormatDto::Excel
                {
                    source_bytes
                } else {
                    u64::try_from(sample.len()).unwrap_or(u64::MAX)
                },
                indexed_rows,
                complete: kind == StructuredViewKindDto::ExternalFallback
                    || request.format == StructuredViewFormatDto::Excel,
                total_rows: (request.format == StructuredViewFormatDto::Excel
                    && kind == StructuredViewKindDto::Table)
                    .then_some(indexed_rows),
                warning: format_warning.clone(),
                checkpoints: vec![CsvCheckpoint { row: 0, byte: 0 }],
                json_checkpoints: vec![JsonCheckpoint {
                    byte: 0,
                    state: JsonLexerState::default(),
                }],
            }),
            hot_pages: Mutex::new(VecDeque::new()),
            cancellation,
            configuration_generation: AtomicU64::new(0),
        });
        self.sessions
            .write()
            .await
            .insert(session_id, Arc::clone(&session));
        if !session.progress.read().await.complete {
            spawn_indexer(Arc::clone(&session), 0);
        }
        let progress = session.progress.read().await;
        Ok(OpenStructuredViewResponseDto {
            session_id,
            kind,
            source_revision: revision,
            source_bytes,
            random_access,
            delimiter: delimiter.map(|value| char::from(value).to_string()),
            header_mode: request.header_mode,
            headers,
            rows,
            sheets,
            selected_sheet,
            indexed_bytes: progress.indexed_bytes,
            indexed_rows: progress.indexed_rows,
            total_rows: progress.total_rows,
            indexing_complete: progress.complete,
            warning: format_warning.or_else(|| {
                (!random_access).then(|| {
                    "This provider is sequential-only; initial rows and forward indexing remain available, but arbitrary jumps are disabled. Open the file externally when random access is required.".to_owned()
                })
            }),
        })
    }

    pub(crate) async fn status(
        &self,
        request: StructuredViewSessionRequestDto,
    ) -> Result<StructuredViewStatusDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        let progress = session.progress.read().await;
        Ok(StructuredViewStatusDto {
            indexed_bytes: progress.indexed_bytes,
            indexed_rows: progress.indexed_rows,
            total_rows: progress.total_rows,
            indexing_complete: progress.complete,
            warning: progress.warning.clone(),
        })
    }

    pub(crate) async fn close(
        &self,
        request: StructuredViewSessionRequestDto,
    ) -> Result<(), ApplicationError> {
        let session = self
            .sessions
            .write()
            .await
            .remove(&request.session_id)
            .ok_or(ApplicationError::NotFound)?;
        session.cancellation.cancel();
        Ok(())
    }

    pub(crate) async fn update(
        &self,
        request: UpdateStructuredViewRequestDto,
    ) -> Result<OpenStructuredViewResponseDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        if session.format == StructuredViewFormatDto::Excel {
            let selected_sheet = request.selected_sheet.ok_or_else(|| {
                ApplicationError::InvalidRequest(
                    "a worksheet name is required for workbook sessions".to_owned(),
                )
            })?;
            let (headers, rows, sheets, row_count) = {
                let workbook_lock = session.workbook.as_ref().ok_or_else(|| {
                    ApplicationError::InvalidRequest(
                        "this workbook was opened as an external fallback".to_owned(),
                    )
                })?;
                let mut workbook = workbook_lock.write().await;
                let selected = workbook
                    .sheets
                    .iter()
                    .position(|sheet| sheet.metadata.name == selected_sheet)
                    .ok_or_else(|| {
                        ApplicationError::InvalidRequest(format!(
                            "worksheet '{selected_sheet}' does not exist"
                        ))
                    })?;
                workbook.selected = selected;
                let sheet = &workbook.sheets[selected];
                (
                    spreadsheet_headers(sheet.metadata.column_count),
                    spreadsheet_page(sheet, 0, INITIAL_ROWS as u16),
                    workbook
                        .sheets
                        .iter()
                        .map(|item| item.metadata.clone())
                        .collect::<Vec<_>>(),
                    sheet.metadata.row_count,
                )
            };
            *session.headers.write().await = headers.clone();
            *session.sample_rows.write().await = rows.clone();
            session.hot_pages.lock().await.clear();
            {
                let mut progress = session.progress.write().await;
                progress.indexed_bytes = session.source_bytes;
                progress.indexed_rows = row_count;
                progress.total_rows = Some(row_count);
                progress.complete = true;
            }
            return Ok(OpenStructuredViewResponseDto {
                session_id: request.session_id,
                kind: StructuredViewKindDto::Table,
                source_revision: session.revision.clone(),
                source_bytes: session.source_bytes,
                random_access: session.random_access,
                delimiter: None,
                header_mode: StructuredHeaderModeDto::None,
                headers,
                rows,
                sheets,
                selected_sheet: Some(selected_sheet),
                indexed_bytes: session.source_bytes,
                indexed_rows: row_count,
                total_rows: Some(row_count),
                indexing_complete: true,
                warning: None,
            });
        }
        if !matches!(
            session.format,
            StructuredViewFormatDto::Csv | StructuredViewFormatDto::Tsv
        ) {
            return Err(ApplicationError::InvalidRequest(
                "delimiter and header options apply only to CSV/TSV sessions".to_owned(),
            ));
        }
        let delimiter = match request.delimiter {
            Some(value) => Some(parse_delimiter(&value)?),
            None => *session.delimiter.read().await,
        };
        let header_mode = request
            .header_mode
            .unwrap_or(*session.header_mode.read().await);
        let sample = read_source_range(
            &session.provider,
            &session.entry,
            session.random_access,
            0,
            session.source_bytes.min(SAMPLE_BYTES),
            session.cancellation.child_token(),
        )
        .await?;
        let (headers, rows, has_header) = parse_initial_delimited_rows(
            &sample,
            delimiter.expect("CSV session delimiter"),
            header_mode,
        )
        .await?;
        *session.delimiter.write().await = delimiter;
        *session.header_mode.write().await = header_mode;
        *session.has_header.write().await = has_header;
        *session.headers.write().await = headers.clone();
        *session.sample_rows.write().await = rows.clone();
        session.hot_pages.lock().await.clear();
        let generation = session
            .configuration_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        {
            let mut progress = session.progress.write().await;
            *progress = Progress {
                indexed_bytes: u64::try_from(sample.len()).unwrap_or(u64::MAX),
                indexed_rows: u64::try_from(rows.len()).unwrap_or(u64::MAX),
                checkpoints: vec![CsvCheckpoint { row: 0, byte: 0 }],
                ..Progress::default()
            };
        }
        spawn_indexer(Arc::clone(&session), generation);
        let progress = session.progress.read().await;
        Ok(OpenStructuredViewResponseDto {
            session_id: request.session_id,
            kind: session.kind,
            source_revision: session.revision.clone(),
            source_bytes: session.source_bytes,
            random_access: session.random_access,
            delimiter: delimiter.map(|value| char::from(value).to_string()),
            header_mode,
            headers,
            rows,
            sheets: Vec::new(),
            selected_sheet: None,
            indexed_bytes: progress.indexed_bytes,
            indexed_rows: progress.indexed_rows,
            total_rows: progress.total_rows,
            indexing_complete: progress.complete,
            warning: progress.warning.clone(),
        })
    }

    pub(crate) async fn read_rows(
        &self,
        request: ReadStructuredRowsRequestDto,
    ) -> Result<ReadStructuredRowsResponseDto, ApplicationError> {
        if request.count == 0 || request.count > MAX_ROWS_PER_REQUEST {
            return Err(ApplicationError::InvalidRequest(format!(
                "row count must be between 1 and {MAX_ROWS_PER_REQUEST}"
            )));
        }
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        if session.kind != StructuredViewKindDto::Table {
            return Err(ApplicationError::InvalidRequest(
                "this session does not expose table rows".to_owned(),
            ));
        }
        let rows = if let Some(rows) = cached_rows(&session, request.start_row, request.count).await
        {
            rows
        } else if let Some(workbook) = &session.workbook {
            let workbook = workbook.read().await;
            spreadsheet_page(
                &workbook.sheets[workbook.selected],
                request.start_row,
                request.count,
            )
        } else {
            read_rows_from_source(&session, request.start_row, request.count).await?
        };
        remember_page(&session, request.start_row, rows.clone()).await;
        let progress = session.progress.read().await;
        Ok(ReadStructuredRowsResponseDto {
            rows,
            indexed_rows: progress.indexed_rows,
            total_rows: progress.total_rows,
            indexing_complete: progress.complete,
        })
    }

    pub(crate) async fn read_json_window(
        &self,
        request: ReadStructuredJsonWindowRequestDto,
    ) -> Result<ReadStructuredJsonWindowResponseDto, ApplicationError> {
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        if session.kind != StructuredViewKindDto::JsonText {
            return Err(ApplicationError::InvalidRequest(
                "this session does not expose JSON text".to_owned(),
            ));
        }
        if request.length == 0 || u64::from(request.length) > SAMPLE_BYTES {
            return Err(ApplicationError::InvalidRequest(format!(
                "JSON window length must be between 1 and {SAMPLE_BYTES} bytes"
            )));
        }
        if request.offset > 0 {
            require_random_access(session.random_access, "JSON window")?;
        }
        let (checkpoint, indexed_bytes, indexing_complete) = {
            let progress = session.progress.read().await;
            (
                progress
                    .json_checkpoints
                    .iter()
                    .rev()
                    .find(|checkpoint| checkpoint.byte <= request.offset)
                    .copied()
                    .unwrap_or(JsonCheckpoint {
                        byte: 0,
                        state: JsonLexerState::default(),
                    }),
                progress.indexed_bytes,
                progress.complete,
            )
        };
        if request.offset > indexed_bytes && !indexing_complete {
            return Err(ApplicationError::InvalidRequest(
                "the requested JSON window has not been indexed yet".to_owned(),
            ));
        }
        let prefix = request.offset.saturating_sub(checkpoint.byte);
        let read_length = prefix
            .saturating_add(u64::from(request.length))
            .saturating_add(4);
        let scanned = read_source_range(
            &session.provider,
            &session.entry,
            session.random_access,
            checkpoint.byte,
            read_length,
            session.cancellation.child_token(),
        )
        .await?;
        let mut visible_start = usize::try_from(prefix)
            .unwrap_or(usize::MAX)
            .min(scanned.len());
        while scanned
            .get(visible_start)
            .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
        {
            visible_start += 1;
        }
        let mut visible_end = visible_start
            .saturating_add(request.length as usize)
            .min(scanned.len());
        while visible_end > visible_start
            && visible_end < scanned.len()
            && scanned
                .get(visible_end)
                .is_some_and(|byte| byte & 0b1100_0000 == 0b1000_0000)
        {
            visible_end = visible_end.saturating_sub(1);
        }
        let mut state = checkpoint.state;
        for byte in &scanned[..visible_start] {
            state.advance(*byte);
        }
        let data = scanned[visible_start..visible_end].to_vec();
        let tokens = json_lexer::lex_with_state(&data, state);
        let offset = checkpoint.byte.saturating_add(visible_start as u64);
        Ok(ReadStructuredJsonWindowResponseDto {
            eof: offset.saturating_add(data.len() as u64) >= session.source_bytes,
            data,
            offset,
            tokens,
        })
    }

    pub(crate) async fn search_rows(
        &self,
        request: SearchStructuredRowsRequestDto,
    ) -> Result<SearchStructuredRowsResponseDto, ApplicationError> {
        if request.query.is_empty() || request.limit == 0 || request.limit > 200 {
            return Err(ApplicationError::InvalidRequest(
                "search query must be non-empty and limit must be between 1 and 200".to_owned(),
            ));
        }
        let session = self.session(request.session_id).await?;
        validate_revision(&session).await?;
        let progress = session.progress.read().await;
        let search_end = progress.indexed_rows;
        let complete = progress.complete;
        drop(progress);
        let mut cursor = request.cursor;
        let query = request.query.to_lowercase();
        let mut matches = Vec::new();
        while cursor < search_end && matches.len() < usize::from(request.limit) {
            if session.cancellation.is_cancelled() {
                return Err(ApplicationError::OperationCancelled);
            }
            let page = self
                .read_rows(ReadStructuredRowsRequestDto {
                    session_id: request.session_id,
                    start_row: cursor,
                    count: 200,
                })
                .await?;
            if page.rows.is_empty() {
                break;
            }
            for row in &page.rows {
                cursor = row.index.saturating_add(1);
                if row
                    .cells
                    .iter()
                    .any(|cell| cell.to_lowercase().contains(&query))
                {
                    matches.push(row.clone());
                    if matches.len() == usize::from(request.limit) {
                        break;
                    }
                }
            }
        }
        Ok(SearchStructuredRowsResponseDto {
            matches,
            next_cursor: (cursor < search_end || !complete).then_some(cursor),
            indexing_complete: complete,
        })
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

async fn load_spreadsheet(
    provider: &Arc<dyn FileSystemProvider>,
    entry: &EntryRef,
    random_access: bool,
    source_bytes: u64,
    cancellation: CancellationToken,
) -> Result<SpreadsheetWorkbook, String> {
    ensure_at_most(source_bytes, WORKBOOK_SOURCE_BYTES, "Workbook source bytes")?;
    if cancellation.is_cancelled() {
        return Err("Workbook parsing was cancelled.".to_owned());
    }
    let bytes = read_source_range(
        provider,
        entry,
        random_access,
        0,
        source_bytes,
        cancellation.clone(),
    )
    .await
    .map_err(|error| format!("The workbook source could not be read: {error}."))?;
    let parsed = tokio::task::spawn_blocking(move || parse_spreadsheet(bytes))
        .await
        .map_err(|error| format!("The workbook parser could not complete: {error}."))??;
    if cancellation.is_cancelled() {
        return Err("Workbook parsing was cancelled.".to_owned());
    }
    Ok(parsed)
}

fn parse_spreadsheet(bytes: Vec<u8>) -> Result<SpreadsheetWorkbook, String> {
    if bytes.starts_with(b"PK\x03\x04") {
        preflight_zip_workbook(&bytes)?;
    }
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|error| format!("The workbook is corrupt or unsupported: {error}."))?;
    let sheet_names = workbook.sheet_names();
    if sheet_names.is_empty() {
        return Err("The workbook contains no worksheets.".to_owned());
    }
    ensure_at_most(
        sheet_names.len() as u64,
        WORKBOOK_SHEET_LIMIT as u64,
        "Workbook sheet count",
    )?;
    if let Some(pictures) = workbook.pictures() {
        let mut total = 0_u64;
        for (_, image) in pictures {
            let size = image.len() as u64;
            ensure_at_most(size, WORKBOOK_IMAGE_BYTES, "Workbook image bytes")?;
            total = total.saturating_add(size);
        }
        ensure_at_most(
            total,
            WORKBOOK_TOTAL_IMAGE_BYTES,
            "Workbook total image bytes",
        )?;
    }

    let mut sheets = Vec::with_capacity(sheet_names.len());
    let mut non_empty_cells = 0_usize;
    let mut retained_bytes = 0_u64;
    for name in sheet_names {
        let values = workbook
            .worksheet_range(&name)
            .map_err(|error| format!("Worksheet '{name}' could not be parsed: {error}."))?;
        let formulas = workbook.worksheet_formula(&name).map_err(|error| {
            format!("Worksheet '{name}' formulas could not be parsed: {error}.")
        })?;
        let value_start = values.start();
        let value_end = values.end();
        let formula_start = formulas.start();
        let formula_end = formulas.end();
        let end_row = value_end
            .map(|position| position.0)
            .into_iter()
            .chain(formula_end.map(|position| position.0))
            .max();
        let end_column = value_end
            .map(|position| position.1)
            .into_iter()
            .chain(formula_end.map(|position| position.1))
            .max();
        let row_count = end_row.map_or(0, |row| u64::from(row) + 1);
        let column_count = end_column.map_or(0, |column| column.saturating_add(1));
        enforce_sheet_dimensions(&name, row_count, column_count)?;

        let mut rows = BTreeMap::<u64, StructuredRowDto>::new();
        let starts = [value_start, formula_start]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let start_row = starts.iter().map(|position| position.0).min().unwrap_or(0);
        let start_column = starts.iter().map(|position| position.1).min().unwrap_or(0);
        for row in start_row..u32::try_from(row_count).unwrap_or(u32::MAX) {
            let mut cells = Vec::new();
            let mut cell_details = Vec::new();
            for column in start_column..column_count {
                let value = values.get_value((row, column)).unwrap_or(&Data::Empty);
                let formula = formulas
                    .get_value((row, column))
                    .filter(|formula| !formula.is_empty());
                if value == &Data::Empty && formula.is_none() {
                    continue;
                }
                non_empty_cells = non_empty_cells.saturating_add(1);
                ensure_at_most(
                    non_empty_cells as u64,
                    WORKBOOK_NON_EMPTY_CELL_LIMIT as u64,
                    "Workbook non-empty cell count",
                )?;
                let display = value.to_string();
                ensure_at_most(
                    display.len() as u64,
                    WORKBOOK_STRING_BYTES as u64,
                    &format!("Cell string bytes in worksheet '{name}'"),
                )?;
                if let Some(formula) = formula {
                    ensure_at_most(
                        formula.len() as u64,
                        WORKBOOK_STRING_BYTES as u64,
                        &format!("Formula string bytes in worksheet '{name}'"),
                    )?;
                }
                retained_bytes = retained_bytes
                    .saturating_add(display.len() as u64)
                    .saturating_add(formula.map_or(0, |formula| formula.len() as u64));
                ensure_at_most(
                    retained_bytes,
                    WORKBOOK_EXPANDED_BYTES,
                    "Parsed workbook string bytes",
                )?;
                let needed = column as usize + 1;
                if cells.len() < needed {
                    cells.resize(needed, String::new());
                }
                cells[column as usize] = display.clone();
                cell_details.push(StructuredCellDto {
                    column,
                    display,
                    value_type: spreadsheet_value_type(value),
                    formula: formula.cloned(),
                });
            }
            if !cell_details.is_empty() {
                rows.insert(
                    u64::from(row),
                    StructuredRowDto {
                        index: u64::from(row),
                        cells,
                        cell_details,
                    },
                );
            }
        }
        sheets.push(SpreadsheetSheet {
            metadata: StructuredSheetDto {
                name,
                row_count,
                column_count,
            },
            rows,
        });
    }
    Ok(SpreadsheetWorkbook {
        sheets,
        selected: 0,
    })
}

fn spreadsheet_value_type(value: &Data) -> StructuredCellValueTypeDto {
    match value {
        Data::Int(_) | Data::Float(_) => StructuredCellValueTypeDto::Number,
        Data::String(_) | Data::Empty => StructuredCellValueTypeDto::Text,
        Data::Bool(_) => StructuredCellValueTypeDto::Boolean,
        Data::DateTime(_) | Data::DateTimeIso(_) => StructuredCellValueTypeDto::DateTime,
        Data::DurationIso(_) => StructuredCellValueTypeDto::Duration,
        Data::Error(_) => StructuredCellValueTypeDto::Error,
    }
}

fn spreadsheet_headers(column_count: u32) -> Vec<String> {
    (0..column_count).map(excel_column_name).collect()
}

fn excel_column_name(mut column: u32) -> String {
    let mut name = String::new();
    loop {
        name.insert(0, char::from(b'A' + (column % 26) as u8));
        if column < 26 {
            return name;
        }
        column = column / 26 - 1;
    }
}

fn spreadsheet_page(sheet: &SpreadsheetSheet, start: u64, count: u16) -> Vec<StructuredRowDto> {
    let end = start
        .saturating_add(u64::from(count))
        .min(sheet.metadata.row_count);
    (start..end)
        .map(|index| {
            sheet.rows.get(&index).cloned().unwrap_or(StructuredRowDto {
                index,
                cells: Vec::new(),
                cell_details: Vec::new(),
            })
        })
        .collect()
}

fn enforce_sheet_dimensions(name: &str, rows: u64, columns: u32) -> Result<(), String> {
    if rows > WORKBOOK_ROWS_PER_SHEET {
        return Err(format!(
            "Worksheet '{name}' has {rows} rows, exceeding the {WORKBOOK_ROWS_PER_SHEET}-row limit."
        ));
    }
    if columns > WORKBOOK_COLUMNS_PER_SHEET {
        return Err(format!(
            "Worksheet '{name}' has {columns} columns, exceeding the {WORKBOOK_COLUMNS_PER_SHEET}-column limit."
        ));
    }
    let dense_cells = rows.saturating_mul(u64::from(columns));
    if dense_cells > WORKBOOK_DENSE_CELL_LIMIT {
        return Err(format!(
            "Worksheet '{name}' spans {dense_cells} cells, exceeding the {WORKBOOK_DENSE_CELL_LIMIT}-cell materialization limit."
        ));
    }
    Ok(())
}

fn ensure_at_most(actual: u64, limit: u64, subject: &str) -> Result<(), String> {
    if actual <= limit {
        Ok(())
    } else {
        Err(format!("{subject} {actual} exceeds the {limit} limit."))
    }
}

fn preflight_zip_workbook(bytes: &[u8]) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("The workbook ZIP package is corrupt: {error}."))?;
    ensure_at_most(
        archive.len() as u64,
        WORKBOOK_ENTRY_LIMIT as u64,
        "Workbook archive entry count",
    )?;
    let mut expanded = 0_u64;
    let mut image_total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Workbook archive entry {index} is corrupt: {error}."))?;
        let size = entry.size();
        expanded = expanded.saturating_add(size);
        ensure_at_most(expanded, WORKBOOK_EXPANDED_BYTES, "Workbook expanded bytes")?;
        let name = entry.name().to_owned();
        if name.starts_with("xl/media/") {
            ensure_at_most(
                size,
                WORKBOOK_IMAGE_BYTES,
                &format!("Workbook image '{name}' bytes"),
            )?;
            image_total = image_total.saturating_add(size);
            ensure_at_most(
                image_total,
                WORKBOOK_TOTAL_IMAGE_BYTES,
                "Workbook total image bytes",
            )?;
        }
        if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut entry, &mut xml)
                .map_err(|error| format!("Worksheet XML '{name}' is invalid UTF-8: {error}."))?;
            if let Some((rows, columns)) = worksheet_declared_dimensions(&xml)? {
                enforce_sheet_dimensions(&name, rows, columns)?;
            }
        }
    }
    Ok(())
}

fn worksheet_declared_dimensions(xml: &str) -> Result<Option<(u64, u32)>, String> {
    let Some(dimension) = xml.find("<dimension") else {
        return Ok(None);
    };
    let tail = &xml[dimension..];
    let Some(reference) = tail.find("ref=\"") else {
        return Ok(None);
    };
    let value = &tail[reference + 5..];
    let end = value
        .find('"')
        .ok_or_else(|| "Worksheet dimension reference is malformed.".to_owned())?;
    let last = value[..end].rsplit(':').next().unwrap_or_default();
    parse_excel_cell_reference(last).map(Some)
}

fn parse_excel_cell_reference(reference: &str) -> Result<(u64, u32), String> {
    let split = reference
        .find(|character: char| character.is_ascii_digit())
        .ok_or_else(|| format!("Worksheet dimension '{reference}' is malformed."))?;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty()
        || digits.is_empty()
        || !letters
            .chars()
            .all(|character| character.is_ascii_alphabetic())
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return Err(format!("Worksheet dimension '{reference}' is malformed."));
    }
    let mut column = 0_u32;
    for letter in letters.bytes() {
        column = column
            .checked_mul(26)
            .and_then(|value| value.checked_add(u32::from(letter.to_ascii_uppercase() - b'A' + 1)))
            .ok_or_else(|| format!("Worksheet dimension '{reference}' is too large."))?;
    }
    let row = digits
        .parse::<u64>()
        .map_err(|_| format!("Worksheet dimension '{reference}' is malformed."))?;
    Ok((row, column))
}

fn source_revision(summary: &fm_domain::EntrySummary, size: u64) -> String {
    format!(
        "{size}:{}:{}",
        summary
            .modified_at
            .and_then(|value| value.timestamp_nanos_opt())
            .unwrap_or_default(),
        summary.metadata_revision
    )
}

async fn validate_revision(session: &Session) -> Result<(), ApplicationError> {
    let summary = session
        .provider
        .inspect(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let size = session
        .provider
        .file_size(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let actual = source_revision(&summary, size);
    if actual != session.revision {
        session.cancellation.cancel();
        return Err(ApplicationError::FileRevisionConflict {
            expected_revision: session.revision.clone(),
            actual_revision: actual,
        });
    }
    Ok(())
}

fn delimiter_for(
    request: &OpenStructuredViewRequestDto,
    sample: &[u8],
) -> Result<Option<u8>, ApplicationError> {
    if let Some(delimiter) = &request.delimiter {
        return parse_delimiter(delimiter).map(Some);
    }
    Ok(match request.format {
        StructuredViewFormatDto::Csv => Some(detect_delimiter(sample)),
        StructuredViewFormatDto::Tsv => Some(b'\t'),
        _ => None,
    })
}

fn parse_delimiter(value: &str) -> Result<u8, ApplicationError> {
    let bytes = value.as_bytes();
    if bytes.len() != 1 || !matches!(bytes[0], b',' | b';' | b'\t' | b'|') {
        return Err(ApplicationError::InvalidRequest(
            "delimiter must be one of comma, semicolon, tab, or pipe".to_owned(),
        ));
    }
    Ok(bytes[0])
}

fn detect_delimiter(sample: &[u8]) -> u8 {
    let sample = sample.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(sample);
    let mut scores = [(b',', 0_u32), (b';', 0), (b'\t', 0), (b'|', 0)];
    let mut quoted = false;
    let mut records = 0_u8;
    let mut index = 0;
    while index < sample.len() && records < 20 {
        match sample[index] {
            b'"' if quoted && sample.get(index + 1) == Some(&b'"') => index += 1,
            b'"' => quoted = !quoted,
            b'\n' if !quoted => records += 1,
            byte if !quoted => {
                if let Some((_, score)) =
                    scores.iter_mut().find(|(candidate, _)| *candidate == byte)
                {
                    *score += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    scores
        .into_iter()
        .max_by_key(|(_, score)| *score)
        .map(|(delimiter, _)| delimiter)
        .unwrap_or(b',')
}

async fn parse_initial_delimited_rows(
    sample: &[u8],
    delimiter: u8,
    header_mode: StructuredHeaderModeDto,
) -> Result<(Vec<String>, Vec<StructuredRowDto>, bool), ApplicationError> {
    let sample = sample.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(sample);
    let reader: ProviderReadStream = Box::pin(Cursor::new(sample.to_vec()));
    let records = parse_delimited_records(reader, delimiter, INITIAL_ROWS + 1).await?;
    let use_header = match header_mode {
        StructuredHeaderModeDto::FirstRow => true,
        StructuredHeaderModeDto::None => false,
        StructuredHeaderModeDto::Auto => records.first().is_some_and(|row| looks_like_header(row)),
    };
    let headers = use_header
        .then(|| records.first().cloned())
        .flatten()
        .unwrap_or_default();
    let start = usize::from(use_header);
    let rows = records
        .into_iter()
        .skip(start)
        .take(INITIAL_ROWS)
        .enumerate()
        .map(|(index, cells)| StructuredRowDto {
            index: index as u64,
            cells,
            cell_details: Vec::new(),
        })
        .collect();
    Ok((headers, rows, use_header))
}

fn looks_like_header(row: &[String]) -> bool {
    !row.is_empty()
        && row.iter().all(|cell| !cell.trim().is_empty())
        && row.iter().any(|cell| cell.chars().any(char::is_alphabetic))
        && row.iter().all(|cell| cell.parse::<f64>().is_err())
}

async fn parse_delimited_records(
    reader: ProviderReadStream,
    delimiter: u8,
    limit: usize,
) -> Result<Vec<Vec<String>>, ApplicationError> {
    let mut parser = AsyncReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .create_reader(reader);
    let mut stream = parser.records();
    let mut rows = Vec::with_capacity(limit.min(INITIAL_ROWS + 1));
    while rows.len() < limit {
        let Some(record) = stream.next().await else {
            break;
        };
        let record = record.map_err(|_| {
            ApplicationError::InvalidRequest(
                "the delimited file contains invalid UTF-8 or malformed quoting".to_owned(),
            )
        })?;
        rows.push(record.iter().map(ToOwned::to_owned).collect());
    }
    Ok(rows)
}

fn parse_initial_ndjson_rows(sample: &[u8]) -> (Vec<String>, Vec<StructuredRowDto>) {
    let rows = String::from_utf8_lossy(sample)
        .lines()
        .take(INITIAL_ROWS)
        .enumerate()
        .map(|(index, value)| StructuredRowDto {
            index: index as u64,
            cells: vec![value.to_owned()],
            cell_details: Vec::new(),
        })
        .collect();
    (vec!["JSON object".to_owned()], rows)
}

async fn read_source_range(
    provider: &Arc<dyn FileSystemProvider>,
    entry: &EntryRef,
    random_access: bool,
    offset: u64,
    length: u64,
    cancellation: CancellationToken,
) -> Result<Vec<u8>, ApplicationError> {
    let reader = if random_access {
        provider
            .read_range(entry, offset, Some(length), cancellation.clone())
            .await
            .map_err(ApplicationError::from)?
    } else {
        let mut reader = provider
            .open_read(entry, cancellation.clone())
            .await
            .map_err(ApplicationError::from)?;
        let mut remaining = offset;
        let mut discard = [0_u8; 64 * 1024];
        while remaining > 0 {
            let amount = remaining.min(discard.len() as u64) as usize;
            let read = reader
                .read(&mut discard[..amount])
                .await
                .map_err(read_stream_error)?;
            if read == 0 {
                break;
            }
            remaining -= read as u64;
        }
        reader
    };
    let mut data = Vec::with_capacity(length.min(SAMPLE_BYTES) as usize);
    reader
        .take(length)
        .read_to_end(&mut data)
        .await
        .map_err(read_stream_error)?;
    Ok(data)
}

fn spawn_indexer(session: Arc<Session>, generation: u64) {
    tokio::spawn(async move {
        let result = match session.format {
            StructuredViewFormatDto::Csv | StructuredViewFormatDto::Tsv => {
                index_delimited(Arc::clone(&session), generation).await
            }
            StructuredViewFormatDto::Ndjson => index_ndjson(Arc::clone(&session)).await,
            StructuredViewFormatDto::Json => index_json(Arc::clone(&session)).await,
            StructuredViewFormatDto::Excel => Ok(()),
        };
        if let Err(error) = result {
            let mut progress = session.progress.write().await;
            if !session.cancellation.is_cancelled()
                && session.configuration_generation.load(Ordering::Acquire) == generation
            {
                progress.warning = Some(error.to_string());
            }
        }
    });
}

async fn index_delimited(session: Arc<Session>, generation: u64) -> Result<(), ApplicationError> {
    let reader = session
        .provider
        .open_read(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let delimiter = session.delimiter.read().await.expect("CSV delimiter");
    let header_rows = u64::from(*session.has_header.read().await);
    let mut parser = AsyncReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .create_reader(reader);
    let mut records = parser.byte_records();
    let mut physical_row = 0_u64;
    while let Some(record) = records.next().await {
        if session.cancellation.is_cancelled()
            || session.configuration_generation.load(Ordering::Acquire) != generation
        {
            return Err(ApplicationError::OperationCancelled);
        }
        let record = record.map_err(|_| {
            ApplicationError::InvalidRequest(
                "CSV indexing stopped at a malformed record".to_owned(),
            )
        })?;
        let data_row = physical_row.saturating_sub(header_rows);
        let byte = record.position().map_or(0, csv_async::Position::byte);
        let mut progress = session.progress.write().await;
        progress.indexed_bytes = byte;
        progress.indexed_rows = physical_row.saturating_add(1).saturating_sub(header_rows);
        if physical_row >= header_rows && data_row.is_multiple_of(SPARSE_ROW_STRIDE) {
            progress.checkpoints.push(CsvCheckpoint {
                row: data_row,
                byte,
            });
        }
        drop(progress);
        physical_row = physical_row.saturating_add(1);
    }
    let mut progress = session.progress.write().await;
    progress.indexed_bytes = session.source_bytes;
    progress.indexed_rows = physical_row.saturating_sub(header_rows);
    progress.total_rows = Some(progress.indexed_rows);
    progress.complete = true;
    Ok(())
}

async fn index_ndjson(session: Arc<Session>) -> Result<(), ApplicationError> {
    index_lines_or_bytes(session, true).await
}

async fn index_json(session: Arc<Session>) -> Result<(), ApplicationError> {
    let mut reader = session
        .provider
        .open_read(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    let mut state = JsonLexerState::default();
    loop {
        if session.cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let read = reader.read(&mut buffer).await.map_err(read_stream_error)?;
        if read == 0 {
            break;
        }
        let mut checkpoints = Vec::new();
        for (index, byte) in buffer[..read].iter().copied().enumerate() {
            let absolute = bytes.saturating_add(index as u64);
            if absolute > 0 && absolute.is_multiple_of(JSON_CHECKPOINT_STRIDE) {
                checkpoints.push(JsonCheckpoint {
                    byte: absolute,
                    state,
                });
            }
            state.advance(byte);
        }
        bytes = bytes.saturating_add(read as u64);
        let mut progress = session.progress.write().await;
        progress.indexed_bytes = bytes;
        progress.json_checkpoints.extend(checkpoints);
    }
    let mut progress = session.progress.write().await;
    progress.indexed_bytes = session.source_bytes;
    progress.complete = true;
    Ok(())
}

async fn index_lines_or_bytes(
    session: Arc<Session>,
    count_lines: bool,
) -> Result<(), ApplicationError> {
    let mut reader = session
        .provider
        .open_read(&session.entry, session.cancellation.child_token())
        .await
        .map_err(ApplicationError::from)?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut bytes = 0_u64;
    let mut rows = 0_u64;
    loop {
        if session.cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        let read = reader.read(&mut buffer).await.map_err(read_stream_error)?;
        if read == 0 {
            break;
        }
        if count_lines {
            let chunk_start = bytes;
            let mut checkpoints = Vec::new();
            for (index, byte) in buffer[..read].iter().enumerate() {
                if *byte == b'\n' {
                    rows = rows.saturating_add(1);
                    if rows.is_multiple_of(SPARSE_ROW_STRIDE) {
                        checkpoints.push(CsvCheckpoint {
                            row: rows,
                            byte: chunk_start.saturating_add(index as u64).saturating_add(1),
                        });
                    }
                }
            }
            if !checkpoints.is_empty() {
                session
                    .progress
                    .write()
                    .await
                    .checkpoints
                    .extend(checkpoints);
            }
        }
        bytes = bytes.saturating_add(read as u64);
        let mut progress = session.progress.write().await;
        progress.indexed_bytes = bytes;
        progress.indexed_rows = rows;
    }
    let mut progress = session.progress.write().await;
    progress.indexed_bytes = session.source_bytes;
    progress.indexed_rows = rows;
    progress.total_rows = count_lines.then_some(rows);
    progress.complete = true;
    Ok(())
}

async fn cached_rows(session: &Session, start: u64, count: u16) -> Option<Vec<StructuredRowDto>> {
    let sample = session.sample_rows.read().await;
    let end = start.saturating_add(u64::from(count));
    if end <= sample.len() as u64 {
        return Some(sample[start as usize..end as usize].to_vec());
    }
    drop(sample);
    session
        .hot_pages
        .lock()
        .await
        .iter()
        .find(|page| start >= page.start && end <= page.start + page.rows.len() as u64)
        .map(|page| {
            let relative = (start - page.start) as usize;
            page.rows[relative..relative + usize::from(count)].to_vec()
        })
}

async fn remember_page(session: &Session, start: u64, rows: Vec<StructuredRowDto>) {
    let mut pages = session.hot_pages.lock().await;
    pages.retain(|page| page.start != start);
    pages.push_front(HotPage { start, rows });
    pages.truncate(HOT_PAGE_LIMIT);
}

async fn read_rows_from_source(
    session: &Session,
    start: u64,
    count: u16,
) -> Result<Vec<StructuredRowDto>, ApplicationError> {
    require_random_access(session.random_access, "row")?;
    if session.format == StructuredViewFormatDto::Ndjson {
        return read_ndjson_rows(session, start, count).await;
    }
    let checkpoint = {
        let progress = session.progress.read().await;
        progress
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.row <= start)
            .copied()
            .unwrap_or(CsvCheckpoint { row: 0, byte: 0 })
    };
    let mut reader = session
        .provider
        .read_range(
            &session.entry,
            checkpoint.byte,
            None,
            session.cancellation.child_token(),
        )
        .await
        .map_err(ApplicationError::from)?;
    if checkpoint.byte == 0 {
        let mut bom = [0_u8; 3];
        let read = reader.read(&mut bom).await.map_err(read_stream_error)?;
        if read != 3 || bom != [0xef, 0xbb, 0xbf] {
            reader = session
                .provider
                .read_range(&session.entry, 0, None, session.cancellation.child_token())
                .await
                .map_err(ApplicationError::from)?;
        }
    }
    let delimiter = session.delimiter.read().await.expect("CSV delimiter");
    let skip_header = checkpoint.row == 0
        && checkpoint.byte == 0
        && *session.header_mode.read().await != StructuredHeaderModeDto::None;
    let records = parse_delimited_records(
        reader,
        delimiter,
        (start - checkpoint.row) as usize + usize::from(count) + usize::from(skip_header),
    )
    .await?;
    let skip = (start - checkpoint.row) as usize + usize::from(skip_header);
    Ok(records
        .into_iter()
        .skip(skip)
        .take(usize::from(count))
        .enumerate()
        .map(|(offset, cells)| StructuredRowDto {
            index: start + offset as u64,
            cells,
            cell_details: Vec::new(),
        })
        .collect())
}

fn require_random_access(random_access: bool, target: &str) -> Result<(), ApplicationError> {
    if random_access {
        Ok(())
    } else {
        Err(ApplicationError::InvalidRequest(format!(
            "arbitrary {target} jumps are unavailable on this sequential-only provider; open the file externally for random access"
        )))
    }
}

async fn read_ndjson_rows(
    session: &Session,
    start: u64,
    count: u16,
) -> Result<Vec<StructuredRowDto>, ApplicationError> {
    let checkpoint = {
        let progress = session.progress.read().await;
        progress
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.row <= start)
            .copied()
            .unwrap_or(CsvCheckpoint { row: 0, byte: 0 })
    };
    let reader = session
        .provider
        .read_range(
            &session.entry,
            checkpoint.byte,
            None,
            session.cancellation.child_token(),
        )
        .await
        .map_err(ApplicationError::from)?;
    let mut lines = BufReader::new(reader).lines();
    let mut current = checkpoint.row;
    let mut rows = Vec::with_capacity(usize::from(count));
    while let Some(line) = lines.next_line().await.map_err(read_stream_error)? {
        if current >= start && rows.len() < usize::from(count) {
            rows.push(StructuredRowDto {
                index: current,
                cells: vec![line],
                cell_details: Vec::new(),
            });
        }
        current = current.saturating_add(1);
        if rows.len() == usize::from(count) {
            break;
        }
    }
    Ok(rows)
}

mod json_lexer {
    use super::JsonLexerState;
    use fm_transport_dto::{JsonTokenKindDto, JsonTokenSpanDto};

    #[cfg(test)]
    pub(super) fn lex(data: &[u8]) -> Vec<JsonTokenSpanDto> {
        lex_with_state(data, JsonLexerState::default())
    }

    pub(super) fn lex_with_state(data: &[u8], mut state: JsonLexerState) -> Vec<JsonTokenSpanDto> {
        let mut tokens = Vec::new();
        let mut index = 0;
        if state.in_string {
            while index < data.len() && state.in_string {
                state.advance(data[index]);
                index += 1;
            }
            tokens.push(JsonTokenSpanDto {
                kind: JsonTokenKindDto::String,
                start: 0,
                length: index as u32,
            });
        }
        while index < data.len() {
            let start = index;
            let kind = match data[index] {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    index += 1;
                    continue;
                }
                b'{' | b'}' | b'[' | b']' | b':' | b',' => {
                    index += 1;
                    JsonTokenKindDto::Punctuation
                }
                b'"' => {
                    index += 1;
                    let mut escaped = false;
                    while index < data.len() {
                        let byte = data[index];
                        index += 1;
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            break;
                        }
                    }
                    let mut lookahead = index;
                    while data.get(lookahead).is_some_and(u8::is_ascii_whitespace) {
                        lookahead += 1;
                    }
                    if data.get(lookahead) == Some(&b':') {
                        JsonTokenKindDto::Property
                    } else {
                        JsonTokenKindDto::String
                    }
                }
                b'-' | b'0'..=b'9' => {
                    index += 1;
                    while data.get(index).is_some_and(|byte| {
                        matches!(byte, b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                    }) {
                        index += 1;
                    }
                    JsonTokenKindDto::Number
                }
                _ if data[start..].starts_with(b"true") || data[start..].starts_with(b"false") => {
                    index += if data[start..].starts_with(b"true") {
                        4
                    } else {
                        5
                    };
                    JsonTokenKindDto::Boolean
                }
                _ if data[start..].starts_with(b"null") => {
                    index += 4;
                    JsonTokenKindDto::Null
                }
                _ => {
                    index += 1;
                    JsonTokenKindDto::Invalid
                }
            };
            tokens.push(JsonTokenSpanDto {
                kind,
                start: start as u32,
                length: (index - start) as u32,
            });
        }
        tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn generated_dense_xlsx(rows: u32, columns: u32) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let static_entries = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Dense" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
        ];
        for (name, data) in static_entries {
            archive.start_file(name, options).expect("start XLSX entry");
            archive
                .write_all(data.as_bytes())
                .expect("write XLSX entry");
        }
        archive
            .start_file("xl/worksheets/sheet1.xml", options)
            .expect("start worksheet");
        write!(
            archive,
            r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:{}{}"/><sheetData>"#,
            excel_column_name(columns - 1),
            rows
        )
        .expect("write worksheet header");
        for row in 1..=rows {
            write!(archive, r#"<row r="{row}">"#).expect("write row");
            for column in 0..columns {
                write!(
                    archive,
                    r#"<c r="{}{row}"><v>{}</v></c>"#,
                    excel_column_name(column),
                    u64::from(row) * u64::from(columns) + u64::from(column)
                )
                .expect("write cell");
            }
            archive.write_all(b"</row>").expect("finish row");
        }
        archive
            .write_all(b"</sheetData></worksheet>")
            .expect("finish worksheet");
        archive
            .finish()
            .expect("finish generated workbook")
            .into_inner()
    }

    #[test]
    fn detects_supported_dialects_outside_quoted_fields() {
        assert_eq!(detect_delimiter(b"a;b\n\"x,y\";z\n"), b';');
        assert_eq!(detect_delimiter(b"a\tb\n1\t2\n"), b'\t');
        assert_eq!(detect_delimiter(b"a|b\n1|2\n"), b'|');
    }

    #[test]
    fn json_lexer_keeps_multibyte_content_inside_one_string_span() {
        let input = br#"{"city":"Z\u00fcrich","ok":true}"#;
        let tokens = json_lexer::lex(input);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == fm_transport_dto::JsonTokenKindDto::Property)
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == fm_transport_dto::JsonTokenKindDto::Boolean)
        );
    }

    #[test]
    fn sequential_providers_report_an_explicit_jump_limitation() {
        assert!(require_random_access(true, "row").is_ok());
        let error = require_random_access(false, "row").expect_err("jump must be rejected");
        assert!(error.to_string().contains("sequential-only provider"));
    }

    #[test]
    fn spreadsheet_dimension_budgets_accept_boundaries_and_reject_the_next_value() {
        assert!(
            enforce_sheet_dimensions(
                "Boundary",
                WORKBOOK_ROWS_PER_SHEET,
                (WORKBOOK_DENSE_CELL_LIMIT / WORKBOOK_ROWS_PER_SHEET) as u32,
            )
            .is_ok()
        );
        assert!(
            enforce_sheet_dimensions("Rows", WORKBOOK_ROWS_PER_SHEET + 1, 1)
                .expect_err("row boundary must be enforced")
                .contains("row limit")
        );
        assert!(
            enforce_sheet_dimensions("Columns", 1, WORKBOOK_COLUMNS_PER_SHEET + 1)
                .expect_err("column boundary must be enforced")
                .contains("column limit")
        );
        assert!(
            enforce_sheet_dimensions(
                "Cells",
                WORKBOOK_DENSE_CELL_LIMIT / u64::from(WORKBOOK_COLUMNS_PER_SHEET) + 1,
                WORKBOOK_COLUMNS_PER_SHEET,
            )
            .expect_err("cell boundary must be enforced")
            .contains("materialization limit")
        );
    }

    #[test]
    fn spreadsheet_scalar_budgets_accept_each_boundary_and_reject_the_next_value() {
        let budgets = [
            ("source bytes", WORKBOOK_SOURCE_BYTES),
            ("expanded bytes", WORKBOOK_EXPANDED_BYTES),
            ("archive entries", WORKBOOK_ENTRY_LIMIT as u64),
            ("sheet count", WORKBOOK_SHEET_LIMIT as u64),
            ("non-empty cells", WORKBOOK_NON_EMPTY_CELL_LIMIT as u64),
            ("string bytes", WORKBOOK_STRING_BYTES as u64),
            ("image bytes", WORKBOOK_IMAGE_BYTES),
            ("total image bytes", WORKBOOK_TOTAL_IMAGE_BYTES),
        ];
        for (name, limit) in budgets {
            assert!(ensure_at_most(limit, limit, name).is_ok(), "{name}");
            assert!(
                ensure_at_most(limit + 1, limit, name)
                    .expect_err("next value must exceed the budget")
                    .contains(name),
                "{name}"
            );
        }
    }

    #[test]
    fn xlsx_preflight_rejects_adversarial_declared_dimensions() {
        assert_eq!(
            worksheet_declared_dimensions(r#"<worksheet><dimension ref="A1:B9"/></worksheet>"#)
                .expect("valid dimension"),
            Some((9, 2))
        );
        let inflated = format!(
            r#"<worksheet><dimension ref="A1:A{}"/></worksheet>"#,
            WORKBOOK_ROWS_PER_SHEET + 1
        );
        let (rows, columns) = worksheet_declared_dimensions(&inflated)
            .expect("valid but inflated dimension")
            .expect("dimension present");
        assert!(enforce_sheet_dimensions("sheet.xml", rows, columns).is_err());
    }

    #[test]
    #[ignore = "explicit peak-RSS probe for task 0172 spreadsheet budgets"]
    fn generated_dense_workbook_peak_memory_probe() {
        let bytes = generated_dense_xlsx(20_000, 20);
        assert!(bytes.len() as u64 <= WORKBOOK_SOURCE_BYTES);
        let workbook = parse_spreadsheet(bytes).expect("generated workbook fits every budget");
        assert_eq!(workbook.sheets[0].metadata.row_count, 20_000);
        assert_eq!(workbook.sheets[0].metadata.column_count, 20);
        assert_eq!(workbook.sheets[0].rows.len(), 20_000);
    }

    #[tokio::test]
    async fn csv_parser_accepts_flexible_rows_and_rejects_invalid_utf8() {
        let (_, rows, _) = parse_initial_delimited_rows(
            b"a,b\n1\n2,3,4\n",
            b',',
            StructuredHeaderModeDto::FirstRow,
        )
        .await
        .expect("flexible records are valid CSV");
        assert_eq!(rows[0].cells, ["1"]);
        assert_eq!(rows[1].cells, ["2", "3", "4"]);

        let malformed =
            parse_initial_delimited_rows(b"a,b\n1,\xff\n", b',', StructuredHeaderModeDto::FirstRow)
                .await;
        assert!(matches!(
            malformed,
            Err(ApplicationError::InvalidRequest(_))
        ));
    }
}
