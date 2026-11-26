//! Terminal sessions as records in the notes folder (task 0358): when a session is compacted, or
//! closed after the model answered, its summary is written to `sessions/` of the notes folder,
//! where `search_notes` finds it in later sessions. Only when the person turned it on and a notes
//! folder is set: the caller passes the writer only then. A failure never affects the session.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use super::session::{SessionInfo, Sessions};
use crate::agent_host::ModelSideCalls;
use crate::chat::ChatGenerator;
use crate::notes::{SessionHost, SessionRecord, SessionRecords};

/// Longest wait for the summary of a closed session; after it the record is skipped.
const CLOSE_SUMMARY_TIMEOUT: Duration = Duration::from_secs(90);

/// Writes the record of session `id` with `summary`, as the session is now.
pub(crate) async fn record_session(
    records: &SessionRecords,
    sessions: &Sessions,
    id: &str,
    summary: &str,
) {
    if let Some(info) = sessions.record_info(id) {
        write(records, id, info, summary).await;
    }
}

/// Forgets a tab's session at once; with `records`, its summary is written in the background
/// (one model call, bounded by [`CLOSE_SUMMARY_TIMEOUT`]) when the model answered at least once.
/// Returns the background task, for callers that want to wait for it. Must run inside the async
/// runtime when `records` is given.
pub(crate) fn close<G>(
    generator: Arc<G>,
    sessions: &Sessions,
    id: &str,
    records: Option<SessionRecords>,
) -> Option<tokio::task::JoinHandle<()>>
where
    G: ChatGenerator + 'static,
{
    let info = sessions.record_info(id);
    sessions.close(id);
    let (records, info) = (records?, info?);
    if !info.answered {
        return None;
    }
    let id = id.to_owned();
    Some(tokio::spawn(async move {
        let side_calls = ModelSideCalls::new(&*generator, CancellationToken::new()).await;
        let summary = tokio::time::timeout(
            CLOSE_SUMMARY_TIMEOUT,
            fm_agent::summarize_conversation(&info.messages, &side_calls, &|_| {}),
        )
        .await;
        match summary {
            Ok(Ok(summary)) => write(&records, &id, info, &summary).await,
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "a closed session could not be summarised");
            }
            Err(_) => tracing::warn!("summarising a closed session took too long; not recorded"),
        }
    }))
}

async fn write(records: &SessionRecords, id: &str, info: SessionInfo, summary: &str) {
    let record = SessionRecord {
        id: id.to_owned(),
        host: SessionHost::Terminal,
        title: info.title,
        folder: info.folder,
        started: info.started,
        summary: summary.to_owned(),
        questions: info.questions,
        files: info.files,
    };
    if let Err(error) = records.write(&record, Utc::now()).await {
        tracing::warn!(error = %error, "the session record could not be written");
    }
}
