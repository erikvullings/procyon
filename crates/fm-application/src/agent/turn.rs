//! One turn of an agent (task 0339): ask the model, stream the answer, run the tool it calls,
//! tell it the result, and ask again, until it answers in plain text or a tool needs the person.
//! The host supplies the model and the conversation; the loop owns the bounds, the streaming of
//! the answer without the tool-call lines, and the steps shown to the person.

use async_trait::async_trait;
use fm_transport_dto::{ChatFailureDto, NoteProposalDto};

use super::stream::LineStreamer;
use super::{ApprovalRequest, ToolContext, ToolOutcome, ToolRegistry};
use crate::file_assistant::truncate_to_bytes;

/// Most tool calls in one turn.
pub(crate) const MAX_STEPS: usize = 24;
/// Longest answer kept, in bytes.
pub(crate) const MAX_REPLY_BYTES: usize = 8 * 1024;

/// The model and the conversation of one agent.
#[async_trait]
pub(crate) trait TurnHost: Send {
    /// The model's next reply to the conversation so far, reported piece by piece to `on_delta`.
    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto>;
    fn push_assistant(&mut self, text: &str);
    fn push_tool(&mut self, text: &str);
    fn is_cancelled(&self) -> bool;
}

/// Progress the person sees while the turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnEvent {
    /// A finished line of the answer.
    Line(String),
    /// A tool ran.
    Step(String),
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnEnd {
    /// The model answered in plain text.
    Answer(String),
    Cancelled,
    StepLimit,
    /// A note waits for the person's review.
    Proposal(NoteProposalDto),
    /// A tool needs the person's approval. The call is not yet in the conversation's result: the
    /// caller records what happened and may run the turn again.
    NeedsApproval(ApprovalRequest),
}

/// Runs tool calls until the turn ends. Every tool run adds its step to `steps`.
pub(crate) async fn run_turn(
    host: &mut impl TurnHost,
    registry: &ToolRegistry,
    ctx: &ToolContext<'_>,
    steps: &mut Vec<String>,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> Result<TurnEnd, ChatFailureDto> {
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
        let Some(call) = registry.parse(trimmed) else {
            return Ok(TurnEnd::Answer(trimmed.to_owned()));
        };
        if host.is_cancelled() {
            return Ok(TurnEnd::Cancelled);
        }
        match call.tool.run(ctx, call.args).await {
            ToolOutcome::Output { step, text } => {
                on_event(TurnEvent::Step(step.clone()));
                steps.push(step);
                host.push_tool(&text);
            }
            ToolOutcome::Proposal(proposal) => return Ok(TurnEnd::Proposal(proposal)),
            ToolOutcome::NeedsApproval(request) => return Ok(TurnEnd::NeedsApproval(request)),
        }
    }
    Ok(TurnEnd::StepLimit)
}

#[cfg(test)]
mod tests;
