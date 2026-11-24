use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use super::*;
use crate::{Message, SkillCatalogue, Typed, TypedTool};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure;

/// A model that replies from a script; it can also answer as if the endpoint called tools
/// natively (the host writes native calls back as `TOOL {"name":..,"arguments":..}` lines).
struct Scripted {
    replies: VecDeque<String>,
    log: Vec<String>,
    /// The conversation as the loop recorded it.
    messages: Vec<Message>,
    cancelled: bool,
    /// Messages the person sends, each once this many replies were generated.
    steering: Vec<(usize, String)>,
    generated: usize,
}

impl Scripted {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|reply| (*reply).to_owned()).collect(),
            log: Vec::new(),
            messages: Vec::new(),
            cancelled: false,
            steering: Vec::new(),
            generated: 0,
        }
    }

    fn results(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter_map(|entry| entry.strip_prefix("tool: "))
            .collect()
    }

    fn notes(&self) -> Vec<&str> {
        self.log
            .iter()
            .filter_map(|entry| entry.strip_prefix("note: "))
            .collect()
    }
}

#[async_trait]
impl TurnHost for Scripted {
    type Error = Failure;

    async fn generate(
        &mut self,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, Failure> {
        let reply = self.replies.pop_front().ok_or(Failure)?;
        self.generated += 1;
        for piece in reply.as_bytes().chunks(4) {
            on_delta(std::str::from_utf8(piece).unwrap_or(""));
        }
        Ok(reply)
    }

    fn push_assistant(&mut self, text: &str, calls: &[ToolCallRecord]) {
        self.log.push(format!("assistant: {text}"));
        self.messages.push(Message::Assistant {
            text: text.to_owned(),
            calls: calls.to_vec(),
        });
    }

    fn push_result(&mut self, call: &ToolCallRecord, text: &str) {
        self.log.push(format!("tool: {text}"));
        self.messages.push(Message::result(call, text));
    }

    fn push_note(&mut self, text: &str) {
        self.log.push(format!("note: {text}"));
        self.messages.push(Message::Note(text.to_owned()));
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn take_steering(&mut self) -> Vec<String> {
        let (ready, later) = std::mem::take(&mut self.steering)
            .into_iter()
            .partition::<Vec<_>, _>(|(after, _)| *after <= self.generated);
        self.steering = later;
        ready
            .into_iter()
            .map(|(_, text)| {
                self.log.push(format!("person: {text}"));
                self.messages.push(Message::User(text.clone()));
                text
            })
            .collect()
    }
}

#[derive(Deserialize, JsonSchema)]
struct Word {
    /// The word.
    word: String,
}

/// A read-only tool that takes a while, counting how many runs overlap.
struct Shout {
    running: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

#[async_trait]
impl TypedTool<Approval> for Shout {
    type Args = Word;

    fn name(&self) -> &'static str {
        "shout"
    }

    fn description(&self) -> &'static str {
        "Shouts."
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::ReadOnly
    }

    async fn run_typed(&self, ctx: &ToolContext<'_>, args: Word) -> ToolOutcome<Approval> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        ctx.report(format!("Shouting {}", args.word));
        // Later words finish first, so an unordered collector would scramble the results.
        let delay = 30u64.saturating_sub(args.word.len() as u64 * 10);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        ToolOutcome::output("Shouted", args.word.to_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Approval(String);

/// A tool that stops the turn for the person.
struct Ask;

#[async_trait]
impl TypedTool<Approval> for Ask {
    type Args = Word;

    fn name(&self) -> &'static str {
        "ask"
    }

    fn description(&self) -> &'static str {
        "Asks the person."
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: Word) -> ToolOutcome<Approval> {
        ToolOutcome::Interrupt(Approval(args.word))
    }
}

struct Harness {
    registry: ToolRegistry<Approval>,
    most_concurrent: Arc<AtomicUsize>,
}

fn harness() -> Harness {
    let most = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry
        .register(Typed(Shout {
            running: Arc::new(AtomicUsize::new(0)),
            most: Arc::clone(&most),
        }))
        .register(Typed(Ask));
    Harness {
        registry,
        most_concurrent: most,
    }
}

struct Run {
    end: Result<TurnEnd<Approval>, Failure>,
    steps: Vec<String>,
    events: Vec<TurnEvent>,
}

async fn turn_with(host: &mut Scripted, harness: &Harness, options: &TurnOptions) -> Run {
    let skills = SkillCatalogue::default();
    let ctx = ToolContext::new(&skills);
    let mut events = Vec::new();
    let end = run_turn(host, &harness.registry, &ctx, options, &mut |event| {
        events.push(event)
    })
    .await;
    let steps = events
        .iter()
        .filter_map(|event| match event {
            TurnEvent::Step(step) => Some(step.clone()),
            TurnEvent::Line(_) | TurnEvent::Progress(_) | TurnEvent::Steered(_) => None,
        })
        .collect();
    Run { end, steps, events }
}

async fn turn(host: &mut Scripted) -> Run {
    turn_with(host, &harness(), &TurnOptions::default()).await
}

#[tokio::test]
async fn a_tool_result_goes_back_to_the_model_until_it_answers() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"hi"}"#, "Done.\nBye."]);
    let run = turn(&mut host).await;

    assert_eq!(run.end, Ok(TurnEnd::Answer("Done.\nBye.".into())));
    assert_eq!(
        run.events,
        [
            TurnEvent::Progress("Shouting hi".into()),
            TurnEvent::Step("Shouted".into()),
            TurnEvent::Line("Done.".into()),
            TurnEvent::Line("Bye.".into()),
        ]
    );
    assert_eq!(host.results(), ["HI"]);
}

#[tokio::test]
async fn a_native_call_written_back_as_a_line_runs_the_same_tool() {
    let line = crate::tool_call_line(None, "shout", &serde_json::json!({"word": "hey"}));
    let mut host = Scripted::new(&[line.as_str(), "Done."]);
    let run = turn(&mut host).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer("Done.".into())));
    assert_eq!(host.results(), ["HEY"]);
}

#[tokio::test]
async fn read_only_calls_of_one_reply_run_together_and_report_in_call_order() {
    let harness = harness();
    let mut host = Scripted::new(&[
        "TOOL {\"tool\":\"shout\",\"word\":\"a\"}\nTOOL {\"tool\":\"shout\",\"word\":\"bb\"}\n\
         TOOL {\"tool\":\"shout\",\"word\":\"ccc\"}",
        "Done.",
    ]);
    let run = turn_with(&mut host, &harness, &TurnOptions::default()).await;

    assert_eq!(run.end, Ok(TurnEnd::Answer("Done.".into())));
    assert_eq!(host.results(), ["A", "BB", "CCC"]);
    assert_eq!(run.steps.len(), 3);
    assert!(harness.most_concurrent.load(Ordering::SeqCst) > 1);
}

#[tokio::test]
async fn the_concurrency_limit_is_kept() {
    let harness = harness();
    let calls = (0..6)
        .map(|n| {
            format!(
                "TOOL {{\"tool\":\"shout\",\"word\":\"{}\"}}",
                "x".repeat(n % 3 + 1)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut host = Scripted::new(&[calls.as_str(), "Done."]);
    let options = TurnOptions {
        max_concurrent_calls: 2,
        ..TurnOptions::default()
    };
    turn_with(&mut host, &harness, &options).await;
    assert_eq!(harness.most_concurrent.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_interrupt_ends_the_turn_and_says_later_calls_did_not_run() {
    let mut host = Scripted::new(&[
        "TOOL {\"tool\":\"shout\",\"word\":\"a\"}\nTOOL {\"tool\":\"ask\",\"word\":\"ok?\"}\n\
         TOOL {\"tool\":\"shout\",\"word\":\"b\"}",
    ]);
    let run = turn(&mut host).await;
    let Ok(TurnEnd::Interrupted { call, interrupt }) = run.end else {
        panic!("{:?}", run.end);
    };
    assert_eq!(interrupt, Approval("ok?".into()));
    assert_eq!(call.name, "ask");
    assert_eq!(call.id, "call_0_1");
    let results = host.results();
    assert_eq!(results[0], "A");
    assert!(results[1].starts_with("Not run"));
    assert_eq!(results.len(), 2);
    // Every recorded call but the interrupting one has its result; the host answers that one.
    let Message::Assistant { calls, .. } = &host.messages[0] else {
        panic!("the reply comes first");
    };
    let answered: Vec<&str> = host.messages[1..]
        .iter()
        .filter_map(|message| match message {
            Message::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(answered, ["call_0_0", "call_0_2"]);
}

#[tokio::test]
async fn native_call_ids_are_recorded_with_the_reply_and_its_results() {
    let lines = [
        crate::tool_call_line(Some("c-1"), "shout", &serde_json::json!({"word": "a"})),
        crate::tool_call_line(Some("c-2"), "shout", &serde_json::json!({"word": "b"})),
    ]
    .join("\n");
    let mut host = Scripted::new(&[lines.as_str(), "Done."]);
    turn(&mut host).await;
    let Message::Assistant { calls, .. } = &host.messages[0] else {
        panic!("the reply comes first");
    };
    let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
    assert_eq!(ids, ["c-1", "c-2"]);
    assert_eq!(
        host.messages[1],
        Message::ToolResult {
            call_id: "c-1".into(),
            name: "shout".into(),
            text: "A".into(),
        }
    );
    assert!(matches!(&host.messages[2], Message::ToolResult { call_id, .. } if call_id == "c-2"));
    assert!(matches!(&host.messages[3], Message::Assistant { calls, .. } if calls.is_empty()));
}

#[tokio::test]
async fn the_result_filter_sees_every_tool_result() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"secret"}"#, "ok"]);
    let options = TurnOptions {
        filter_result: Some(|text| text.replace("SECRET", "[redacted]")),
        ..TurnOptions::default()
    };
    turn_with(&mut host, &harness(), &options).await;
    assert_eq!(host.results(), ["[redacted]"]);
}

#[tokio::test]
async fn a_promise_of_more_work_without_a_call_is_sent_back_to_the_model() {
    let mut host = Scripted::new(&[
        r#"TOOL {"tool":"shout","word":"a"}"#,
        "Read the first file. I will continue with the remaining files.",
        r#"TOOL {"tool":"shout","word":"b"}"#,
        "All done.",
    ]);
    let run = turn(&mut host).await;

    assert_eq!(run.end, Ok(TurnEnd::Answer("All done.".into())));
    assert_eq!(run.steps, ["Shouted", "Shouted"]);
    assert!(
        host.notes()
            .iter()
            .any(|text| text.contains("made no tool call"))
    );
}

#[tokio::test]
async fn a_promise_before_any_tool_use_or_forever_is_not_nudged_endlessly() {
    let run = turn(&mut Scripted::new(&["I will continue later."])).await;
    assert_eq!(
        run.end,
        Ok(TurnEnd::Answer("I will continue later.".into()))
    );

    let mut host = Scripted::new(&[
        r#"TOOL {"tool":"shout","word":"a"}"#,
        "I will continue.",
        "I will continue.",
        "I will continue.",
        "I will continue.",
    ]);
    let run = turn(&mut host).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer("I will continue.".into())));
    let nudges = host
        .notes()
        .iter()
        .filter(|text| text.contains("made no tool call"))
        .count();
    assert_eq!(nudges, 3);
}

#[tokio::test]
async fn without_a_nudge_a_promise_is_the_answer() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"a"}"#, "I will continue."]);
    let options = TurnOptions {
        nudge: None,
        ..TurnOptions::default()
    };
    let run = turn_with(&mut host, &harness(), &options).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer("I will continue.".into())));
}

#[tokio::test]
async fn tool_call_lines_are_not_streamed_to_the_person() {
    let mut host = Scripted::new(&[
        "Let me look.\nTOOL {\"tool\":\"shout\",\"word\":\"x\"}",
        "ok",
    ]);
    let run = turn(&mut host).await;
    assert!(
        run.events
            .iter()
            .all(|event| !matches!(event, TurnEvent::Line(line) if line.contains("TOOL")))
    );
    assert!(run.events.contains(&TurnEvent::Line("Let me look.".into())));
}

#[tokio::test]
async fn a_held_line_that_was_no_call_is_shown_after_all() {
    let mut host = Scripted::new(&["{\"name\": \"Ada\", \"age\": 36} is the record."]);
    let run = turn(&mut host).await;
    assert!(run.events.contains(&TurnEvent::Line(
        "{\"name\": \"Ada\", \"age\": 36} is the record.".into()
    )));
}

#[tokio::test]
async fn bad_arguments_are_an_error_for_the_model_not_the_end() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout"}"#, "Sorry."]);
    let run = turn(&mut host).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer("Sorry.".into())));
    assert!(
        host.results()
            .iter()
            .any(|text| text.contains("invalid arguments for shout") && text.contains("word"))
    );
}

#[tokio::test]
async fn a_turn_stops_after_the_step_limit() {
    let options = TurnOptions::default();
    let calls = vec![r#"TOOL {"tool":"shout","word":"a"}"#; options.max_steps + 5];
    let mut host = Scripted::new(&calls);
    let run = turn_with(&mut host, &harness(), &options).await;
    assert_eq!(run.end, Ok(TurnEnd::StepLimit));
    assert_eq!(run.steps.len(), options.max_steps);
}

#[tokio::test]
async fn cancelling_stops_before_the_model_is_asked() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"a"}"#]);
    host.cancelled = true;
    let run = turn(&mut host).await;
    assert_eq!(run.end, Ok(TurnEnd::Cancelled));
    assert!(run.steps.is_empty());
    assert_eq!(host.replies.len(), 1);
}

#[tokio::test]
async fn an_empty_reply_and_a_host_error_end_the_turn() {
    let run = turn(&mut Scripted::new(&["  \n"])).await;
    assert_eq!(run.end, Ok(TurnEnd::EmptyReply));
    let run = turn(&mut Scripted::new(&[])).await;
    assert_eq!(run.end, Err(Failure));
}

#[tokio::test]
async fn a_long_answer_is_kept_whole_unless_the_host_caps_it() {
    let long = "a".repeat(64 * 1024);
    let run = turn(&mut Scripted::new(&[long.as_str()])).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer(long.clone())));

    let options = TurnOptions {
        max_answer_bytes: Some(1024),
        ..TurnOptions::default()
    };
    let run = turn_with(&mut Scripted::new(&[long.as_str()]), &harness(), &options).await;
    assert_eq!(run.end, Ok(TurnEnd::Answer("a".repeat(1024))));
}

#[test]
fn a_promise_is_found_only_at_the_end_of_a_reply() {
    assert!(announces_more_work(
        "Read one. I’ll continue with the next."
    ));
    assert!(!announces_more_work(&format!(
        "I will continue. {}",
        "x".repeat(400)
    )));
}

#[tokio::test]
async fn a_huge_tool_result_is_cut_before_the_model_sees_it() {
    let word = "w".repeat(5000);
    let call = format!("TOOL {{\"tool\":\"shout\",\"word\":\"{word}\"}}");
    let mut host = Scripted::new(&[call.as_str(), "ok"]);
    let options = TurnOptions {
        max_result_bytes: Some(100),
        ..TurnOptions::default()
    };
    turn_with(&mut host, &harness(), &options).await;
    let result = host.results()[0];
    assert!(result.starts_with(&"W".repeat(100)));
    assert!(result.ends_with("[The result was cut at 100 bytes.]"));
}

#[tokio::test]
async fn a_call_after_prose_on_one_line_is_not_streamed_raw() {
    let mut host = Scripted::new(&[r#"Let me look: {"tool":"shout","word":"x"}"#, "ok"]);
    let run = turn(&mut host).await;
    assert_eq!(host.results(), ["X"]);
    assert!(
        run.events
            .iter()
            .all(|event| !matches!(event, TurnEvent::Line(line) if line.contains("\"tool\"")))
    );
}

#[tokio::test]
async fn what_a_running_tool_reports_is_shown_before_its_step() {
    let mut host = Scripted::new(&[
        "TOOL {\"tool\":\"shout\",\"word\":\"a\"}\nTOOL {\"tool\":\"shout\",\"word\":\"bb\"}",
        "Done.",
    ]);
    let run = turn(&mut host).await;
    assert!(matches!(run.end, Ok(TurnEnd::Answer(_))), "{:?}", run.end);
    let shown: Vec<&TurnEvent> = run
        .events
        .iter()
        .filter(|event| !matches!(event, TurnEvent::Line(_)))
        .collect();
    // Both calls run together and report while running; their steps follow, in call order.
    let (reports, steps) = shown.split_at(2);
    let mut reports: Vec<&TurnEvent> = reports.to_vec();
    reports.sort_by_key(|event| format!("{event:?}"));
    assert_eq!(
        reports,
        [
            &TurnEvent::Progress("Shouting a".into()),
            &TurnEvent::Progress("Shouting bb".into()),
        ]
    );
    assert_eq!(
        steps,
        [
            &TurnEvent::Step("Shouted".into()),
            &TurnEvent::Step("Shouted".into()),
        ]
    );
}

#[tokio::test]
async fn a_message_sent_during_tool_calls_reaches_the_model_after_them() {
    let mut host = Scripted::new(&[r#"TOOL {"tool":"shout","word":"a"}"#, "Done."]);
    host.steering.push((1, "also do b".into()));
    let run = turn(&mut host).await;

    assert_eq!(run.end, Ok(TurnEnd::Answer("Done.".into())));
    // The result of the running call comes first; the message never cuts into a round.
    let order: Vec<&str> = host
        .log
        .iter()
        .map(String::as_str)
        .filter(|entry| entry.starts_with("tool:") || entry.starts_with("person:"))
        .collect();
    assert_eq!(order, ["tool: A", "person: also do b"]);
    assert!(run.events.contains(&TurnEvent::Steered("also do b".into())));
}

#[tokio::test]
async fn a_message_sent_while_the_answer_is_written_is_answered_in_the_same_turn() {
    let mut host = Scripted::new(&["Done.", "Done, and b too."]);
    host.steering.push((1, "and b?".into()));
    let run = turn(&mut host).await;

    assert_eq!(run.end, Ok(TurnEnd::Answer("Done, and b too.".into())));
    assert_eq!(
        run.events
            .iter()
            .filter(|event| matches!(event, TurnEvent::Steered(_)))
            .count(),
        1
    );
}
