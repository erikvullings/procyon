//! One turn of an agent (task 0339): ask the model, stream the answer, run the tool it calls,
//! tell it the result, and ask again, until it answers in plain text or a tool needs the person.
//! The host supplies the model and the conversation; the loop owns the bounds, the streaming of
//! the answer without the tool-call lines, and the steps shown to the person.

use async_trait::async_trait;
use fm_transport_dto::{ChatFailureDto, NoteProposalDto};

use super::stream::LineStreamer;
use super::{ApprovalRequest, ToolContext, ToolOutcome, ToolRegistry};
use crate::text::truncate_to_bytes;

/// Most tool calls in one turn.
pub(crate) const MAX_STEPS: usize = 24;
/// Longest answer kept, in bytes.
pub const MAX_REPLY_BYTES: usize = 8 * 1024;

/// The model and the conversation of one agent.
#[async_trait]
pub trait TurnHost: Send {
    /// The model's next reply to the conversation so far, reported piece by piece to `on_delta`.
    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto>;
    /// Records the model's reply in the conversation.
    fn push_assistant(&mut self, text: &str);
    /// Records a tool result in the conversation.
    fn push_tool(&mut self, text: &str);
    /// Whether the person stopped the request.
    fn is_cancelled(&self) -> bool;
}

/// Progress the person sees while the turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// A finished line of the answer.
    Line(String),
    /// A tool ran.
    Step(String),
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEnd {
    /// The model answered in plain text.
    Answer(String),
    /// The person stopped the request.
    Cancelled,
    /// The model kept calling tools until the step limit.
    StepLimit,
    /// A note waits for the person's review.
    Proposal(NoteProposalDto),
    /// A tool needs the person's approval. The call is not yet in the conversation's result: the
    /// caller records what happened and may run the turn again.
    NeedsApproval(ApprovalRequest),
}

/// Most times a reply that only promises more work is sent back to the model in one turn.
const MAX_NUDGES: usize = 3;

/// Whether the end of a reply promises further work ("I will continue with the remaining files").
fn announces_more_work(reply: &str) -> bool {
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
    let tail: String = reply
        .chars()
        .rev()
        .take(240)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let tail = tail.to_lowercase().replace('\u{2019}', "'");
    PROMISES.iter().any(|promise| tail.contains(promise))
}

/// Runs tool calls until the turn ends. Every tool run adds its step to `steps`.
pub async fn run_turn(
    host: &mut impl TurnHost,
    registry: &ToolRegistry,
    ctx: &ToolContext<'_>,
    steps: &mut Vec<String>,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> Result<TurnEnd, ChatFailureDto> {
    let mut worked = false;
    let mut nudges = 0;
    for _ in 0..MAX_STEPS {
        let mut streamer = LineStreamer::new();
        let raw = host
            .generate(&mut |delta: &str| {
                streamer.push(delta, &mut |text| {
                    on_event(TurnEvent::Line(text.to_owned()))
                });
            })
            .await?;
        streamer.finish(&mut |text| on_event(TurnEvent::Line(text.to_owned())));
        // A reply that looked like a tool call but was not one is shown after all.
        if registry.parse(&raw).is_some() {
            streamer.discard_held();
        } else {
            streamer.release_held(&mut |text| on_event(TurnEvent::Line(text.to_owned())));
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ChatFailureDto::Empty);
        }
        let trimmed = truncate_to_bytes(trimmed, MAX_REPLY_BYTES);
        host.push_assistant(trimmed);
        let calls = registry.parse_all(trimmed);
        if calls.is_empty() {
            // A model that has been using tools may announce more work and then stop; ask it to
            // go on instead of ending the task on a promise.
            if worked && nudges < MAX_NUDGES && announces_more_work(trimmed) {
                nudges += 1;
                host.push_tool(
                    "You said you would continue, but you made no tool call, so nothing happened. \
                     Call the next tool now. Only if the whole task is complete, give the final \
                     result.",
                );
                continue;
            }
            return Ok(TurnEnd::Answer(trimmed.to_owned()));
        }
        worked = true;
        let total = calls.len();
        for (index, call) in calls.into_iter().enumerate() {
            if host.is_cancelled() {
                return Ok(TurnEnd::Cancelled);
            }
            match call.tool.run(ctx, call.args).await {
                ToolOutcome::Output { step, text } => {
                    on_event(TurnEvent::Step(step.clone()));
                    steps.push(step);
                    host.push_tool(&text);
                }
                outcome => {
                    if index + 1 < total {
                        host.push_tool(
                            "The calls after this one were not run. Ask again for them if they \
                             are still needed.",
                        );
                    }
                    return Ok(match outcome {
                        ToolOutcome::Proposal(proposal) => TurnEnd::Proposal(proposal),
                        ToolOutcome::NeedsApproval(request) => TurnEnd::NeedsApproval(request),
                        ToolOutcome::Output { .. } => unreachable!("handled above"),
                    });
                }
            }
        }
    }
    Ok(TurnEnd::StepLimit)
}

#[cfg(test)]
mod tests;
