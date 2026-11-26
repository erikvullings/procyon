use std::sync::Mutex;

use async_trait::async_trait;

use super::*;
use crate::message::ToolCallRecord;

/// A model that answers with a numbered summary and remembers the prompts.
struct Recorder {
    prompts: Mutex<Vec<String>>,
    fail: bool,
}

impl Recorder {
    fn new() -> Self {
        Self {
            prompts: Mutex::new(Vec::new()),
            fail: false,
        }
    }
}

#[async_trait]
impl SideModel for Recorder {
    fn input_bytes(&self) -> usize {
        64 * 1024
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String, SideModelError> {
        assert!(system.contains("earlier part of a conversation"));
        if self.fail {
            return Err(SideModelError("offline".into()));
        }
        let mut prompts = self.prompts.lock().unwrap();
        prompts.push(prompt.to_owned());
        Ok(format!("summary {}", prompts.len()))
    }
}

/// `count` exchanges: a question, a reply with a call, its result, and an answer.
fn exchanges(count: usize) -> Vec<Message> {
    let mut messages = Vec::new();
    for index in 0..count {
        let call = ToolCallRecord {
            id: format!("c{index}"),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": format!("f{index}.md") }),
        };
        messages.push(Message::User(format!(
            "question {index} {}",
            "q".repeat(200)
        )));
        messages.push(Message::Assistant {
            text: String::new(),
            calls: vec![call.clone()],
        });
        messages.push(Message::result(&call, "r".repeat(400)));
        messages.push(Message::Assistant {
            text: format!("answer {index}"),
            calls: Vec::new(),
        });
    }
    messages
}

async fn run(messages: &[Message], budget: usize, model: &Recorder) -> Option<Compaction> {
    compact(messages, budget, model, &|_| {}).await.unwrap()
}

#[tokio::test]
async fn a_conversation_within_the_threshold_is_left_alone() {
    let messages = exchanges(2);
    let model = Recorder::new();
    let budget = size(&messages) * 100 / COMPACT_AT_PERCENT + 100;
    assert_eq!(run(&messages, budget, &model).await, None);
    assert!(model.prompts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_oldest_exchanges_are_summarised_and_the_newest_kept_whole() {
    let messages = exchanges(10);
    let model = Recorder::new();
    let budget = size(&messages);
    let compaction = run(&messages, budget, &model).await.unwrap();

    // The cut is at a question, so no call is kept without its reply or result.
    assert!(matches!(messages[compaction.covered], Message::User(_)));
    let kept = &messages[compaction.covered..];
    assert!(size(kept) <= budget / 100 * KEEP_AFTER_COMPACTING_PERCENT);
    assert!(compaction.covered >= 4);
    let prompt = &model.prompts.lock().unwrap()[0];
    assert!(prompt.contains("question 0"));
    assert!(!prompt.contains(&format!("question {}", 9)));
    assert_eq!(
        compaction.message(),
        Message::Note(format!("{COMPACTED_INTRO}\nsummary 1"))
    );
}

#[tokio::test]
async fn an_earlier_summary_is_folded_into_the_next() {
    let first = Compaction {
        summary: "the person wants decoy lessons".into(),
        covered: 0,
    };
    let mut messages = vec![first.message()];
    messages.extend(exchanges(10));
    let model = Recorder::new();
    let compaction = run(&messages, size(&messages), &model).await.unwrap();
    let prompt = &model.prompts.lock().unwrap()[0];
    assert!(
        prompt.contains("the person wants decoy lessons"),
        "{prompt}"
    );
    assert!(compaction.covered > 1);
}

#[tokio::test]
async fn a_failed_summary_is_an_error_for_the_host_to_fall_back_from() {
    let messages = exchanges(10);
    let mut model = Recorder::new();
    model.fail = true;
    let result = compact(&messages, size(&messages), &model, &|_| {}).await;
    assert_eq!(result, Err(SideModelError("offline".into())));
}

#[test]
fn only_one_long_question_leaves_nothing_to_summarise() {
    let messages = vec![Message::User("x".repeat(10_000))];
    assert_eq!(compaction_cut(&messages, 100), 0);
}

#[tokio::test]
async fn a_whole_conversation_is_summarised_for_a_record() {
    let model = Recorder::new();
    let messages = vec![
        Message::User("Person: where are the decoys?".into()),
        Message::Assistant {
            text: "In ./decoys.".into(),
            calls: Vec::new(),
        },
    ];
    let summary = summarize_conversation(&messages, &model, &|_| {})
        .await
        .unwrap();
    assert_eq!(summary, "summary 1");
    assert!(model.prompts.lock().unwrap()[0].contains("where are the decoys?"));

    let failing = Recorder {
        fail: true,
        ..Recorder::new()
    };
    assert!(
        summarize_conversation(&messages, &failing, &|_| {})
            .await
            .is_err()
    );
}
