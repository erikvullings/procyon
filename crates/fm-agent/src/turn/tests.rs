use std::collections::VecDeque;

use serde::Deserialize;
use serde_json::json;

use super::*;
use crate::{SkillCatalogue, Typed, TypedTool};

struct Scripted {
    replies: VecDeque<String>,
    log: Vec<String>,
    cancelled: bool,
}

impl Scripted {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|reply| (*reply).to_owned()).collect(),
            log: Vec::new(),
            cancelled: false,
        }
    }
}

#[async_trait]
impl TurnHost for Scripted {
    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, ChatFailureDto> {
        let reply = self
            .replies
            .pop_front()
            .unwrap_or_else(|| "again".to_owned());
        for piece in reply.as_bytes().chunks(4) {
            on_delta(std::str::from_utf8(piece).unwrap_or(""));
        }
        Ok(reply)
    }

    fn push_assistant(&mut self, text: &str) {
        self.log.push(format!("assistant: {text}"));
    }

    fn push_tool(&mut self, text: &str) {
        self.log.push(format!("tool: {text}"));
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

#[derive(Deserialize)]
struct Word {
    word: String,
}

struct Shout;

#[async_trait]
impl TypedTool for Shout {
    type Args = Word;

    fn name(&self) -> &'static str {
        "shout"
    }

    fn description(&self) -> &'static str {
        "shouts."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"word": {"type": "string"}}, "required": ["word"]})
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: Word) -> ToolOutcome {
        ToolOutcome::output("Shouted", args.word.to_uppercase())
    }
}

async fn turn(
    host: &mut Scripted,
) -> (Result<TurnEnd, ChatFailureDto>, Vec<String>, Vec<TurnEvent>) {
    let mut registry = ToolRegistry::default();
    registry.register(Typed(Shout));
    let skills = SkillCatalogue::default();
    let ctx = ToolContext {
        skills: &skills,
        notes: None,
        files: None,
    };
    let mut steps = Vec::new();
    let mut events = Vec::new();
    let end = run_turn(host, &registry, &ctx, &mut steps, &mut |event| {
        events.push(event)
    })
    .await;
    (end, steps, events)
}

#[tokio::test]
async fn a_tool_result_goes_back_to_the_model_until_it_answers() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"hi"}"#, "Done.\nBye."]);
    let (end, steps, events) = turn(&mut host).await;

    assert_eq!(end, Ok(TurnEnd::Answer("Done.\nBye.".into())));
    assert_eq!(steps, ["Shouted"]);
    assert_eq!(
        events,
        [
            TurnEvent::Step("Shouted".into()),
            TurnEvent::Line("Done.".into()),
            TurnEvent::Line("Bye.".into()),
        ]
    );
    assert!(host.log.contains(&"tool: HI".to_owned()));
}

#[tokio::test]
async fn tool_call_lines_are_not_streamed_to_the_person() {
    let mut host = Scripted::new(&[
        "Let me look.\nTOOL {\"tool\":\"shout\",\"word\":\"x\"}",
        "ok",
    ]);
    let (_, _, events) = turn(&mut host).await;
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, TurnEvent::Line(line) if line.contains("TOOL")))
    );
}

#[tokio::test]
async fn bad_arguments_are_an_error_for_the_model_not_the_end() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout"}"#, "Sorry."]);
    let (end, _, _) = turn(&mut host).await;
    assert_eq!(end, Ok(TurnEnd::Answer("Sorry.".into())));
    assert!(
        host.log
            .iter()
            .any(|entry| entry.contains("invalid arguments for shout"))
    );
}

#[tokio::test]
async fn a_turn_stops_after_the_step_limit() {
    let calls = vec![r#"TOOL {"tool":"shout","word":"a"}"#; MAX_STEPS + 5];
    let mut host = Scripted::new(&calls);
    let (end, steps, _) = turn(&mut host).await;
    assert_eq!(end, Ok(TurnEnd::StepLimit));
    assert_eq!(steps.len(), MAX_STEPS);
}

#[tokio::test]
async fn cancelling_stops_before_the_tool_runs() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"a"}"#]);
    host.cancelled = true;
    let (end, steps, _) = turn(&mut host).await;
    assert_eq!(end, Ok(TurnEnd::Cancelled));
    assert!(steps.is_empty());
}

#[tokio::test]
async fn an_empty_reply_fails_and_a_long_answer_is_cut() {
    let (end, _, _) = turn(&mut Scripted::new(&["  \n"])).await;
    assert_eq!(end, Err(ChatFailureDto::Empty));
    let long = "a".repeat(MAX_REPLY_BYTES * 2);
    let (end, _, _) = turn(&mut Scripted::new(&[long.as_str()])).await;
    assert_eq!(end, Ok(TurnEnd::Answer("a".repeat(MAX_REPLY_BYTES))));
}
