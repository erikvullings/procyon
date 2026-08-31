//! Delta-based change tracking: [`fm_vfs::FileSystemProvider::watch`]'s real
//! implementation (task 0110), the exemplar
//! [`fm_vfs::ChangeTracking::DeltaApi`]'s own documentation names.
//!
//! Mirrors `fm-application`'s `poll_change_stream` (its `ChangeTracking::Poll`
//! equivalent) closely: a [`futures::stream::unfold`]-based
//! [`ProviderChangeStream`] whose single step function loops internally
//! until it has something worth emitting, and cancellation is checked
//! before every network call and every sleep. Unlike `poll_change_stream`,
//! not every failure is silently absorbed: a transient I/O failure backs
//! off and retries without emitting anything (`send_with_retry` already
//! retried the throttling cases; an `Io` surfacing this far is a deeper
//! transport hiccup), but a permanent failure - bad credentials, denied
//! permissions, or any other non-`Io` [`fm_vfs::VfsError`] - is surfaced
//! once as a stream `Err` rather than retried forever with no observable
//! signal (task 0110 review). Surfacing an `Err` does not end the stream:
//! a later tick can still succeed once the caller resolves the underlying
//! cause (see [`classify_error`]).

use std::sync::Arc;
use std::time::Duration;

use fm_domain::Location;
use fm_vfs::{ProviderChange, ProviderChangeStream, VfsError};
use futures::stream;
use reqwest::header::AUTHORIZATION;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::graph::{
    self, DeltaPage, DriveItem, GraphConfig, Parsed, RetryClass, bearer_header_value, build_url,
    map_status, same_origin_family, send_with_retry,
};
use crate::resolver::OneDriveConnectionResolver;

struct State {
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    connection_id: String,
    location_text: String,
    parsed: Parsed,
    /// The watched folder's real Graph item id, resolved once on the first
    /// tick for a non-root location (`watch()` itself never resolves it
    /// eagerly, matching the "no upfront I/O" shape of the `Poll` tracking
    /// kind this mirrors). Always `None` and unused for the drive root.
    resolved_item_id: Option<String>,
    /// The retained opaque `@odata.deltaLink`. `None` means "not yet
    /// seeded" - the very next tick seeds it via `token=latest` and
    /// deliberately emits nothing yet.
    cursor: Option<String>,
    cancellation: CancellationToken,
}

/// Builds the [`ProviderChangeStream`] `watch()` returns. Performs no I/O
/// itself beyond parsing `location`; every Graph call happens lazily inside
/// the returned stream's first poll onward.
pub(crate) fn watch(
    http: reqwest::Client,
    config: GraphConfig,
    resolver: Arc<dyn OneDriveConnectionResolver>,
    location: Location,
    cancellation: CancellationToken,
) -> Result<ProviderChangeStream, VfsError> {
    let parsed = Parsed::parse(&location)?;
    let state = State {
        http,
        config,
        resolver,
        connection_id: parsed.connection_id.clone(),
        location_text: location.uri.clone(),
        parsed,
        resolved_item_id: None,
        cursor: None,
        cancellation,
    };
    Ok(Box::pin(stream::unfold(state, step)))
}

async fn step(mut state: State) -> Option<(Result<ProviderChange, VfsError>, State)> {
    loop {
        if state.cancellation.is_cancelled() {
            return None;
        }

        if state.cursor.is_none() {
            if !state.parsed.is_root() && state.resolved_item_id.is_none() {
                match resolve_item_id(&state).await {
                    Ok(id) => state.resolved_item_id = Some(id),
                    Err(error) => match classify_error(&state, error).await {
                        ErrorAction::Cancelled => return None,
                        ErrorAction::Retry => continue,
                        ErrorAction::Permanent(error) => return Some((Err(error), state)),
                    },
                }
            }
            match seed(&state).await {
                Ok(cursor) => state.cursor = Some(cursor),
                Err(error) => match classify_error(&state, error).await {
                    ErrorAction::Cancelled => return None,
                    ErrorAction::Retry => continue,
                    ErrorAction::Permanent(error) => return Some((Err(error), state)),
                },
            }
            // The seed only establishes a baseline cursor - the caller's
            // own listing, taken just before subscribing, already reflects
            // current state, so emitting here would just force an
            // immediate, redundant re-list (mirrors `poll_change_stream`'s
            // identical first-tick convention).
            if sleep_cancellably(state.config.delta_poll_interval, &state.cancellation)
                .await
                .is_err()
            {
                return None;
            }
            continue;
        }

        match poll_round(&state).await {
            Ok(RoundOutcome::Unchanged { new_cursor }) => {
                state.cursor = Some(new_cursor);
            }
            Ok(RoundOutcome::Changed { new_cursor }) => {
                state.cursor = Some(new_cursor);
                return Some((Ok(ProviderChange::Changed), state));
            }
            Ok(RoundOutcome::Reset) => {
                // A 410 means Graph purged the sync state this cursor
                // referenced; the only safe recovery is a fresh
                // `token=latest` reseed (task 0110), never resuming from
                // the stale cursor.
                state.cursor = None;
                return Some((Ok(ProviderChange::ResetRequired), state));
            }
            Err(error) => match classify_error(&state, error).await {
                ErrorAction::Cancelled => return None,
                ErrorAction::Retry => continue,
                ErrorAction::Permanent(error) => return Some((Err(error), state)),
            },
        }
        if sleep_cancellably(state.config.delta_poll_interval, &state.cancellation)
            .await
            .is_err()
        {
            return None;
        }
    }
}

/// What `step` should do after `resolve_item_id`/`seed`/`poll_round` fails.
enum ErrorAction {
    /// Cancellation ended the watch; the stream must end too.
    Cancelled,
    /// A transient failure was backed off; retry the same step again.
    Retry,
    /// A permanent failure must be surfaced as a stream item rather than
    /// silently retried forever - the watch stays alive regardless (a
    /// later tick may succeed again once the caller resolves the
    /// underlying cause, e.g. by re-authenticating).
    Permanent(VfsError),
}

/// Whether `error` describes a transient condition worth silently retrying
/// (task 0110 review: never silently retry a permanent failure forever).
/// Only a bare I/O/protocol failure ([`VfsError::Io`]) is treated as
/// transient here - throttling itself is already retried inside
/// [`send_with_retry`], so an `Io` surfacing this far is a deeper transport
/// or parsing hiccup, not a structural one. Every other variant
/// (credentials, permissions, location shape, ...) describes something
/// retrying the exact same request can never fix on its own, so it must be
/// surfaced rather than silently absorbed.
fn is_transient(error: &VfsError) -> bool {
    matches!(error, VfsError::Io { .. })
}

/// Classifies an error from `resolve_item_id`/`seed`/`poll_round` into an
/// [`ErrorAction`]. Backs off (honouring cancellation) before returning
/// anything except [`ErrorAction::Cancelled`], so even a *permanent*
/// failure cannot be hammered in a tight loop by a caller that keeps
/// polling the stream as fast as possible.
async fn classify_error(state: &State, error: VfsError) -> ErrorAction {
    if matches!(error, VfsError::Cancelled) {
        return ErrorAction::Cancelled;
    }
    if sleep_cancellably(state.config.delta_poll_interval, &state.cancellation)
        .await
        .is_err()
    {
        return ErrorAction::Cancelled;
    }
    if is_transient(&error) {
        ErrorAction::Retry
    } else {
        ErrorAction::Permanent(error)
    }
}

async fn sleep_cancellably(duration: Duration, cancellation: &CancellationToken) -> Result<(), ()> {
    tokio::select! {
        () = cancellation.cancelled() => Err(()),
        () = tokio::time::sleep(duration) => Ok(()),
    }
}

async fn resolve_item_id(state: &State) -> Result<String, VfsError> {
    let token = state.resolver.resolve(&state.connection_id).await?;
    let url = build_url(&state.config, &state.parsed.metadata_relative_path())?;
    let response = send_with_retry(
        || {
            state
                .http
                .get(url.clone())
                .header(AUTHORIZATION, bearer_header_value(&token))
        },
        RetryClass::Idempotent,
        &state.config.retry,
        &state.cancellation,
    )
    .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_status(status, &state.location_text));
    }
    let item: DriveItem = response.json().await.map_err(|_| unparseable_response())?;
    Ok(item.id)
}

/// The initial seed request is the *only* place `$top` is ever attached by
/// this provider: task 0110's opaque-link contract requires every
/// subsequent `@odata.nextLink`/`@odata.deltaLink` to be followed exactly
/// as Graph returned it, never re-appended or reconstructed. The fixture
/// (and real Graph) echoes whatever `$top` it was given back into every
/// link it hands back from here on, so the page size established here
/// keeps flowing through without this provider ever touching a link again.
fn delta_start_relative_path(state: &State) -> String {
    let top = state.config.delta_page_size;
    match &state.resolved_item_id {
        Some(id) => format!(
            "me/drive/items/{}/delta?token=latest&$top={top}",
            graph::percent_encode_component(id)
        ),
        None => format!("me/drive/root/delta?token=latest&$top={top}"),
    }
}

async fn seed(state: &State) -> Result<String, VfsError> {
    let token = state.resolver.resolve(&state.connection_id).await?;
    let url = build_url(&state.config, &delta_start_relative_path(state))?;
    let response = send_with_retry(
        || {
            state
                .http
                .get(url.clone())
                .header(AUTHORIZATION, bearer_header_value(&token))
        },
        RetryClass::Idempotent,
        &state.config.retry,
        &state.cancellation,
    )
    .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(map_status(status, &state.location_text));
    }
    let page: DeltaPage = response.json().await.map_err(|_| unparseable_response())?;
    page.delta_link.ok_or_else(missing_delta_link)
}

enum RoundOutcome {
    Unchanged { new_cursor: String },
    Changed { new_cursor: String },
    Reset,
}

/// Polls the retained delta link, following every `@odata.nextLink` page in
/// this round verbatim (never reconstructed) before returning, so a caller
/// sees at most one coalesced [`ProviderChange::Changed`] per round no
/// matter how many pages of changes accumulated since the last poll.
async fn poll_round(state: &State) -> Result<RoundOutcome, VfsError> {
    let token = state.resolver.resolve(&state.connection_id).await?;
    // The retained cursor is an opaque link - the `@odata.deltaLink` from a
    // previous round's final page, or the seed's - already carrying
    // whatever `$top` was established at seed time (task 0110's opaque
    // contract). It is used exactly as received, byte for byte: no
    // parameter is ever appended, removed or reconstructed here.
    let mut url_text = state
        .cursor
        .clone()
        .expect("poll_round is only called once a cursor exists");
    let mut saw_changes = false;
    loop {
        if !same_origin_family(&url_text, &state.config.base_url) {
            return Err(VfsError::Io {
                message: "Microsoft Graph delta link failed a same-origin safety check".to_owned(),
            });
        }
        let url = Url::parse(&url_text).map_err(|_| VfsError::Io {
            message: "invalid Microsoft Graph delta link".to_owned(),
        })?;
        let response = send_with_retry(
            || {
                state
                    .http
                    .get(url.clone())
                    .header(AUTHORIZATION, bearer_header_value(&token))
            },
            RetryClass::Idempotent,
            &state.config.retry,
            &state.cancellation,
        )
        .await?;
        let status = response.status();
        if status == reqwest::StatusCode::GONE {
            return Ok(RoundOutcome::Reset);
        }
        if !status.is_success() {
            return Err(map_status(status, &state.location_text));
        }
        let page: DeltaPage = response.json().await.map_err(|_| unparseable_response())?;
        if !page.value.is_empty() {
            saw_changes = true;
        }
        if let Some(next) = page.next_link {
            url_text = next;
            continue;
        }
        let delta_link = page.delta_link.ok_or_else(missing_delta_link)?;
        return Ok(if saw_changes {
            RoundOutcome::Changed {
                new_cursor: delta_link,
            }
        } else {
            RoundOutcome::Unchanged {
                new_cursor: delta_link,
            }
        });
    }
}

fn unparseable_response() -> VfsError {
    VfsError::Io {
        message: "Microsoft Graph returned a response this provider could not parse".to_owned(),
    }
}

fn missing_delta_link() -> VfsError {
    VfsError::Io {
        message: "Microsoft Graph delta response did not include a delta link on its final page"
            .to_owned(),
    }
}
