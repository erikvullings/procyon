//! One turn of an agent: ask the model, stream the answer, run the tools it calls, give it the
//! results, and ask again, until it answers in plain text or a tool stops the turn for the person.
//!
//! The host supplies the model and owns the conversation ([`TurnHost`]); the loop owns the bounds
//! ([`TurnOptions`]), the streaming of the answer without the tool-call lines, the order and
//! concurrency of tool runs, and the steps shown to the person.

use std::borrow::Cow;

use async_trait::async_trait;
use std::pin::pin;

use futures::StreamExt;
use futures::channel::mpsc;
use futures::future::{Either, select};
use futures::stream;

use crate::message::ToolCallRecord;
use crate::registry::{ParsedCall, ToolRegistry};
use crate::stream::LineStreamer;
use crate::text::truncate_to_bytes;
use crate::tool::{ToolContext, ToolEffect, ToolOutcome, ToolOutput};

/// The model and the conversation of one agent.
#[async_trait]
pub trait TurnHost: Send {
    /// Why the model could not answer (the host's own error type).
    type Error: Send;

    /// The model's next reply to the conversation so far, reported piece by piece to `on_delta`.
    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, Self::Error>;
    /// Records the model's reply and the calls the loop runs from it. Every recorded call gets
    /// exactly one result: from the loop through [`push_result`](Self::push_result), or, for the
    /// call that interrupted the turn, from the host later.
    fn push_assistant(&mut self, text: &str, calls: &[ToolCallRecord]);
    /// Records the result of one call.
    fn push_result(&mut self, call: &ToolCallRecord, text: &str);
    /// Records a message from the loop to the model (a nudge to go on).
    fn push_note(&mut self, text: &str);
    /// Whether the person stopped the request.
    fn is_cancelled(&self) -> bool;
    /// Messages the person sent while the turn was running (task 0355), oldest first. The host
    /// records each in the conversation as a person message before returning it; the loop asks
    /// after every round of tool calls and before it accepts a final answer, so a message never
    /// interrupts a running tool. The default has none.
    fn take_steering(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// Progress the person sees while the turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// A finished line of the answer.
    Line(String),
    /// A tool ran; its step line.
    Step(String),
    /// What a tool that is still running is doing (see [`ToolContext::report`]). Each report
    /// replaces the one before; the tool's [`Step`](Self::Step) replaces the last.
    Progress(String),
    /// A message the person sent during the turn reached the model ([`TurnHost::take_steering`]).
    Steered(String),
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEnd<I> {
    /// The model answered in plain text.
    Answer(String),
    /// The model replied with nothing.
    EmptyReply,
    /// The person stopped the request.
    Cancelled,
    /// The model kept calling tools until [`TurnOptions::max_steps`].
    StepLimit,
    /// A tool stopped the turn for the person. Its call has no result in the conversation yet: the
    /// host records what happened as that call's result and may run the turn again.
    Interrupted {
        /// The call that stopped the turn.
        call: ToolCallRecord,
        /// What the tool asks of the person.
        interrupt: I,
    },
}

/// Asks a model that announced more work without calling a tool to go on.
#[derive(Debug, Clone)]
pub struct ContinueNudge {
    /// Whether a reply promises more work.
    pub promises_more: fn(&str) -> bool,
    /// What the model is told.
    pub message: Cow<'static, str>,
    /// Most nudges in one turn.
    pub max: usize,
}

impl Default for ContinueNudge {
    fn default() -> Self {
        Self {
            promises_more: announces_more_work,
            message: Cow::Borrowed(
                "You said you would continue, but you made no tool call, so nothing happened. \
                 Call the next tool now. Only if the whole task is complete, give the final result.",
            ),
            max: 3,
        }
    }
}

/// The bounds and messages of a turn.
#[derive(Debug, Clone)]
pub struct TurnOptions {
    /// Most model replies with tool calls in one turn.
    pub max_steps: usize,
    /// Most calls run from one reply; later ones are ignored.
    pub max_calls_per_reply: usize,
    /// Most read-only calls of one reply running at the same time.
    pub max_concurrent_calls: usize,
    /// Longest answer kept, in bytes; `None` keeps it whole.
    pub max_answer_bytes: Option<usize>,
    /// Longest tool result the model sees, in bytes; a longer one is cut with a note. `None`
    /// leaves the bound to the tools and the host.
    pub max_result_bytes: Option<usize>,
    /// Applied to every tool result before the model sees it (for example secret redaction).
    pub filter_result: Option<fn(&str) -> String>,
    /// Asks a model that only announced more work to go on; `None` takes such a reply as the answer.
    pub nudge: Option<ContinueNudge>,
    /// The result of the calls of a reply that did not run because an earlier call stopped the
    /// turn.
    pub skipped_calls_message: Cow<'static, str>,
}

impl Default for TurnOptions {
    fn default() -> Self {
        Self {
            max_steps: 24,
            max_calls_per_reply: 8,
            max_concurrent_calls: 4,
            max_answer_bytes: None,
            max_result_bytes: Some(64 * 1024),
            filter_result: None,
            nudge: Some(ContinueNudge::default()),
            skipped_calls_message: Cow::Borrowed(
                "Not run: an earlier call of this reply stopped for the person. Call it again if it \
                 is still needed.",
            ),
        }
    }
}

/// Whether the end of a reply promises further work ("I will continue with the remaining files").
/// English phrases only; hosts for other languages supply their own [`ContinueNudge`].
pub fn announces_more_work(reply: &str) -> bool {
    const PROMISES: [&str; 9] = [
        "i will continue",
        "i'll continue",
        "let me continue",
        "i will proceed",
        "i'll proceed",
        "i will now",
        "i'll now",
        "i will read the",
        "i will start",
    ];
    let start = reply
        .char_indices()
        .rev()
        .nth(239)
        .map_or(0, |(index, _)| index);
    let tail = reply[start..].to_lowercase().replace('\u{2019}', "'");
    PROMISES.iter().any(|promise| tail.contains(promise))
}

/// Runs model replies and tool calls until the turn ends. `on_event` sees every answer line and
/// every step.
pub async fn run_turn<H, I>(
    host: &mut H,
    registry: &ToolRegistry<I>,
    ctx: &ToolContext<'_>,
    options: &TurnOptions,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> Result<TurnEnd<I>, H::Error>
where
    H: TurnHost,
    I: Send + 'static,
{
    let mut worked = false;
    let mut nudges = 0;
    for step in 0..options.max_steps {
        if host.is_cancelled() {
            return Ok(TurnEnd::Cancelled);
        }
        let mut streamer = LineStreamer::new();
        let raw = host
            .generate(&mut |delta: &str| {
                streamer.push(delta, &mut |text| {
                    on_event(TurnEvent::Line(text.to_owned()))
                });
            })
            .await?;
        streamer.finish(&mut |text| on_event(TurnEvent::Line(text.to_owned())));
        let reply = raw.trim();
        let calls = registry.parse(reply, options.max_calls_per_reply);
        // Held lines looked like a call; they are shown after all when the reply had none.
        if calls.is_empty() {
            streamer.release_held(&mut |text| on_event(TurnEvent::Line(text.to_owned())));
        } else {
            streamer.discard_held();
        }
        if reply.is_empty() {
            return Ok(TurnEnd::EmptyReply);
        }
        let kept = match options.max_answer_bytes {
            Some(max) if calls.is_empty() => truncate_to_bytes(reply, max),
            _ => reply,
        };
        let records: Vec<ToolCallRecord> = calls
            .iter()
            .enumerate()
            .map(|(index, call)| ToolCallRecord {
                id: call
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("call_{step}_{index}")),
                name: call.tool.name().to_owned(),
                arguments: call.args.clone(),
            })
            .collect();
        host.push_assistant(kept, &records);
        if calls.is_empty() {
            if let Some(nudge) = &options.nudge
                && worked
                && nudges < nudge.max
                && (nudge.promises_more)(kept)
            {
                nudges += 1;
                host.push_note(&nudge.message);
                continue;
            }
            // A message sent while the model wrote its answer is answered in the same turn.
            if deliver_steering(host, on_event) {
                continue;
            }
            return Ok(TurnEnd::Answer(kept.to_owned()));
        }
        worked = true;
        if let Some(end) = run_calls(host, ctx, options, calls, records, on_event).await {
            return Ok(end);
        }
        deliver_steering(host, on_event);
    }
    Ok(TurnEnd::StepLimit)
}

/// Gives the model what the person sent meanwhile; whether there was anything.
fn deliver_steering<H: TurnHost>(
    host: &mut H,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> bool {
    let steered = host.take_steering();
    let any = !steered.is_empty();
    for text in steered {
        on_event(TurnEvent::Steered(text));
    }
    any
}

/// Runs the calls of one reply: consecutive read-only calls together (bounded), the others alone,
/// results recorded in call order. A call that interrupts ends the turn once its batch is done:
/// the calls of the batch that ran keep their results, later calls get the skipped message.
/// `Some` when the turn ends.
async fn run_calls<H, I>(
    host: &mut H,
    ctx: &ToolContext<'_>,
    options: &TurnOptions,
    calls: Vec<ParsedCall<I>>,
    records: Vec<ToolCallRecord>,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> Option<TurnEnd<I>>
where
    H: TurnHost,
    I: Send + 'static,
{
    let mut pending = calls.into_iter().zip(records).peekable();
    let mut stop: Option<(ToolCallRecord, I)> = None;
    while let Some(first) = pending.next() {
        if host.is_cancelled() {
            // The calls that never ran still need their results.
            for (_, record) in std::iter::once(first).chain(pending) {
                host.push_result(&record, crate::message::NOT_RUN_RESULT);
            }
            return Some(TurnEnd::Cancelled);
        }
        let mut batch = vec![first];
        if batch[0].0.tool.effect() == ToolEffect::ReadOnly {
            while let Some(next) =
                pending.next_if(|(call, _)| call.tool.effect() == ToolEffect::ReadOnly)
            {
                batch.push(next);
            }
        }
        let outcomes = run_batch(ctx, options, batch, on_event).await;
        for (outcome, record) in outcomes {
            match outcome {
                ToolOutcome::Output(ToolOutput { step, text }) => {
                    on_event(TurnEvent::Step(step));
                    host.push_result(&record, &result_for_model(options, &text));
                }
                ToolOutcome::Interrupt(interrupt) if stop.is_none() => {
                    stop = Some((record, interrupt));
                }
                // Only one call stops the turn; a second one is skipped.
                ToolOutcome::Interrupt(_) => {
                    host.push_result(&record, &options.skipped_calls_message);
                }
            }
        }
        if let Some((call, interrupt)) = stop {
            for (_, record) in pending {
                host.push_result(&record, &options.skipped_calls_message);
            }
            return Some(TurnEnd::Interrupted { call, interrupt });
        }
    }
    None
}

/// Runs one batch of calls, at most [`TurnOptions::max_concurrent_calls`] at a time, and passes
/// on what they report while they run. Outcomes come back in call order.
async fn run_batch<I: Send + 'static>(
    ctx: &ToolContext<'_>,
    options: &TurnOptions,
    batch: Vec<(ParsedCall<I>, ToolCallRecord)>,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> Vec<(ToolOutcome<I>, ToolCallRecord)> {
    let (sender, mut reports) = mpsc::unbounded::<String>();
    let report = move |text: String| {
        // The receiver outlives every call of the batch; a failed send has no one to tell.
        let _ = sender.unbounded_send(text);
    };
    let call_ctx = ToolContext {
        progress: Some(&report),
        ..*ctx
    };
    let call_ctx = &call_ctx;
    let mut work = pin!(
        stream::iter(batch)
            .map(|(call, record)| async move { (call.tool.run(call_ctx, call.args).await, record) })
            .buffered(options.max_concurrent_calls.max(1))
            .collect::<Vec<_>>()
    );
    loop {
        match select(work.as_mut(), reports.next()).await {
            Either::Left((outcomes, _)) => {
                // Reports sent just before the batch finished still go out, in order.
                while let Ok(text) = reports.try_recv() {
                    on_event(TurnEvent::Progress(text));
                }
                return outcomes;
            }
            Either::Right((Some(text), _)) => on_event(TurnEvent::Progress(text)),
            Either::Right((None, rest)) => return rest.await,
        }
    }
}

/// A tool result as the model sees it: bounded, then filtered.
fn result_for_model(options: &TurnOptions, text: &str) -> String {
    let bounded: Cow<'_, str> = match options.max_result_bytes {
        Some(max) if text.len() > max => Cow::Owned(format!(
            "{}\n[The result was cut at {max} bytes.]",
            truncate_to_bytes(text, max)
        )),
        _ => Cow::Borrowed(text),
    };
    match options.filter_result {
        Some(filter) => filter(&bounded),
        None => bounded.into_owned(),
    }
}

#[cfg(test)]
mod tests;
