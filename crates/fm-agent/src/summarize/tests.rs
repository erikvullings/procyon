use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::skills::SkillCatalogue;
use crate::tool::{AgentTool, FileTools, ToolOutput, Typed};

/// A folder with one file, read whole up to the limit asked for.
struct OneFile {
    name: &'static str,
    text: String,
}

#[async_trait]
impl FileTools for OneFile {
    async fn list_dir(&self, _path: &str) -> ToolOutput {
        ToolOutput::new("Listed", "")
    }

    async fn read_file(&self, _path: &str, _offset: usize) -> ToolOutput {
        ToolOutput::new("Read", self.text.clone())
    }

    async fn grep(&self, _pattern: &str, _path: Option<&str>) -> ToolOutput {
        ToolOutput::new("Searched", "")
    }

    async fn read_whole(&self, path: &str, max: usize) -> Result<WholeText, String> {
        if path != self.name {
            return Err(format!("{path} is not available."));
        }
        let truncated = self.text.len() > max;
        Ok(WholeText {
            text: self.text[..self.text.len().min(max)].to_owned(),
            truncated,
        })
    }
}

/// A model that answers with a numbered summary and remembers the prompts.
struct Recorder {
    input_bytes: usize,
    prompts: Mutex<Vec<String>>,
    fail: bool,
    /// Bytes of filler each answer carries, for summaries that do not all fit one call.
    padding: usize,
}

impl Recorder {
    fn new(input_bytes: usize) -> Self {
        Self {
            input_bytes,
            prompts: Mutex::new(Vec::new()),
            fail: false,
            padding: 0,
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl SideModel for Recorder {
    fn input_bytes(&self) -> usize {
        self.input_bytes
    }

    async fn complete(&self, system: &str, prompt: &str) -> Result<String, SideModelError> {
        assert!(system.contains("summarise one document"));
        if self.fail {
            return Err(SideModelError("timed out".into()));
        }
        let mut prompts = self.prompts.lock().unwrap();
        prompts.push(prompt.to_owned());
        Ok(format!(
            "summary {}{}",
            prompts.len(),
            " y".repeat(self.padding / 2)
        ))
    }
}

async fn run(files: &OneFile, model: &Recorder, args: serde_json::Value) -> ToolOutput {
    run_reporting(files, model, args).await.0
}

/// The output and what the tool reported while it ran.
async fn run_reporting(
    files: &OneFile,
    model: &Recorder,
    args: serde_json::Value,
) -> (ToolOutput, Vec<String>) {
    let skills = SkillCatalogue::default();
    let reports = Mutex::new(Vec::new());
    let report = |text: String| reports.lock().unwrap().push(text);
    let ctx = ToolContext {
        files: Some(files),
        side_model: Some(model),
        progress: Some(&report),
        ..ToolContext::new(&skills)
    };
    let output = match AgentTool::<()>::run(&Typed(SummarizeFile), &ctx, args).await {
        ToolOutcome::Output(output) => output,
        ToolOutcome::Interrupt(()) => panic!("no interrupt expected"),
    };
    (output, reports.into_inner().unwrap())
}

#[tokio::test]
async fn a_short_document_is_summarised_in_one_call_and_only_the_summary_returns() {
    let files = OneFile {
        name: "paper.pdf",
        text: "The secret plan of the paper.".into(),
    };
    let model = Recorder::new(16 * 1024);
    let output = run(
        &files,
        &model,
        json!({ "path": "paper.pdf", "focus": "decoys" }),
    )
    .await;

    assert_eq!(output.step, "Summarised paper.pdf");
    assert_eq!(
        output.text,
        "Summary of paper.pdf (1 KB of text):\nsummary 1"
    );
    let prompts = model.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("Document: paper.pdf\nFocus: decoys\n"));
    assert!(prompts[0].contains("The secret plan of the paper."));
}

#[tokio::test]
async fn a_long_document_is_summarised_in_parts_then_combined() {
    let paragraph = "word ".repeat(1000);
    let files = OneFile {
        name: "long.md",
        text: [paragraph.as_str(); 3].join("\n\n"),
    };
    let model = Recorder::new(MIN_PART_BYTES);
    let (output, reports) = run_reporting(&files, &model, json!({ "path": "long.md" })).await;

    let prompts = model.prompts();
    let parts = prompts.len() - 1;
    assert!(parts >= 3, "{parts} parts");
    assert!(prompts[0].contains(&format!("part 1 of {parts}")));
    let combine = prompts.last().unwrap();
    assert!(combine.contains(&format!("summaries of its {parts} parts")));
    assert!(combine.contains("Part 1:\nsummary 1"));
    assert_eq!(output.step, format!("Summarised long.md ({parts} parts)"));
    assert!(output.text.ends_with(&format!("summary {}", parts + 1)));
    // The person sees each call as it starts.
    let mut expected = vec!["Reading long.md".to_owned()];
    expected.extend((1..=parts).map(|part| format!("Summarising long.md: part {part} of {parts}")));
    expected.push(format!("Summarising long.md: combining {parts} parts"));
    assert_eq!(reports, expected);
}

#[tokio::test]
async fn summaries_too_long_for_one_call_are_combined_in_rounds() {
    let paragraph = "word ".repeat(800);
    let files = OneFile {
        name: "book.pdf",
        text: [paragraph.as_str(); 8].join("\n\n"),
    };
    let mut model = Recorder::new(MIN_PART_BYTES);
    // Three summaries never fit one call, two do: rounds of 8, then 4, then 2.
    model.padding = 1500;
    let (output, reports) = run_reporting(&files, &model, json!({ "path": "book.pdf" })).await;

    let prompts = model.prompts();
    assert_eq!(prompts.len(), 8 + 4 + 2 + 1, "{reports:#?}");
    // No call is given more than one part's worth of summaries.
    for prompt in &prompts[8..] {
        assert!(
            prompt.len() < MIN_PART_BYTES + 256,
            "{} bytes",
            prompt.len()
        );
    }
    assert!(prompts[8].starts_with("Document: book.pdf, parts 1-2 of 8\n"));
    assert!(prompts[8].contains("Part 1:\nsummary 1"));
    assert!(prompts[8].contains("Part 2:\nsummary 2"));
    // The second round combines what the first made, labelled by the parts it covers.
    assert!(prompts[12].starts_with("Document: book.pdf, parts 1-4 of 8\n"));
    assert!(prompts[12].contains("Parts 1-2:\nsummary 9"));
    assert!(prompts[12].contains("Parts 3-4:\nsummary 10"));
    let last = prompts.last().unwrap();
    assert!(last.contains("These are summaries of its 8 parts"));
    assert!(last.contains("Parts 1-4:\nsummary 13"));
    assert!(last.contains("Parts 5-8:\nsummary 14"));
    assert_eq!(output.step, "Summarised book.pdf (8 parts)");
    assert!(output.text.contains("summary 15"));

    let mut expected = vec!["Reading book.pdf".to_owned()];
    expected.extend((1..=8).map(|part| format!("Summarising book.pdf: part {part} of 8")));
    for (first, last) in [(1, 2), (3, 4), (5, 6), (7, 8), (1, 4), (5, 8)] {
        expected.push(format!(
            "Summarising book.pdf: combining parts {first}-{last} of 8"
        ));
    }
    expected.push("Summarising book.pdf: combining 8 parts".to_owned());
    assert_eq!(reports, expected);
}

#[test]
fn a_summary_left_alone_in_a_round_is_carried_to_the_next() {
    let covered = |part: usize, bytes: usize| Covered {
        first: part,
        last: part,
        summary: "z".repeat(bytes),
    };
    let groups = group_summaries(
        vec![covered(1, 1000), covered(2, 1000), covered(3, 1000)],
        2500,
    );
    let parts: Vec<Vec<usize>> = groups
        .iter()
        .map(|group| group.iter().map(|covered| covered.first).collect())
        .collect();
    assert_eq!(parts, vec![vec![1, 2], vec![3]]);
    // Even summaries larger than a call are paired, so every round shrinks.
    let oversized = group_summaries(vec![covered(1, 9000), covered(2, 9000)], 2500);
    assert_eq!(oversized.len(), 1);
}

#[tokio::test]
async fn only_the_start_of_a_very_long_document_is_summarised_and_the_result_says_so() {
    let files = OneFile {
        name: "huge.txt",
        text: "x ".repeat(MIN_PART_BYTES * MAX_SUMMARY_PARTS),
    };
    let model = Recorder::new(MIN_PART_BYTES);
    let output = run(&files, &model, json!({ "path": "huge.txt" })).await;
    assert!(model.prompts().len() <= MAX_SUMMARY_PARTS + 1);
    assert!(
        output.text.contains("the rest was not read"),
        "{}",
        output.text
    );
}

#[tokio::test]
async fn failures_reach_the_model_as_tool_errors() {
    let files = OneFile {
        name: "a.md",
        text: "text".into(),
    };
    let missing = run(&files, &Recorder::new(8192), json!({ "path": "b.md" })).await;
    assert_eq!(missing.text, "Error: b.md is not available.");

    let mut failing = Recorder::new(8192);
    failing.fail = true;
    let failed = run(&files, &failing, json!({ "path": "a.md" })).await;
    assert_eq!(
        failed.text,
        "Error: the model could not summarise a.md: timed out"
    );
    // The person sees that it failed and why, not "Summarised a.md".
    assert_eq!(failed.step, "Could not summarise a.md: timed out");

    let skills = SkillCatalogue::default();
    let without = ToolContext {
        files: Some(&files),
        ..ToolContext::new(&skills)
    };
    let outcome =
        AgentTool::<()>::run(&Typed(SummarizeFile), &without, json!({ "path": "a.md" })).await;
    assert!(
        matches!(outcome, ToolOutcome::Output(output) if output.text.contains("use read_file"))
    );
}

#[test]
fn text_is_split_at_paragraphs_then_lines_and_never_inside_a_character() {
    let text = format!("{}\n\n{}", "a".repeat(60), "b".repeat(60));
    assert_eq!(split_text(&text, 100), [&text[..62], &text[62..]]);
    let lines = format!("{}\n{}", "a".repeat(60), "b".repeat(60));
    assert_eq!(split_text(&lines, 100)[0], &lines[..61]);
    let accents = "é".repeat(10);
    let parts = split_text(&accents, 3);
    assert!(parts.iter().all(|part| !part.is_empty()));
    assert_eq!(parts.concat(), accents);
    assert_eq!(split_text("", 10), [""]);
}
