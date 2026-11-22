use serde::Deserialize;
use serde_json::json;

use super::*;

#[derive(Deserialize)]
struct EchoArgs {
    text: String,
    times: Option<u32>,
}

struct Echo;

#[async_trait]
impl TypedTool for Echo {
    type Args = EchoArgs;

    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "repeats text."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"text": {"type": "string"}, "times": {"type": "integer"}},
            "required": ["text"],
        })
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: EchoArgs) -> ToolOutcome {
        ToolOutcome::output("Echoed", args.text.repeat(args.times.unwrap_or(1) as usize))
    }
}

fn registry() -> ToolRegistry {
    let mut registry = ToolRegistry::default();
    registry.register(Typed(Echo));
    registry
}

fn context(skills: &SkillCatalogue) -> ToolContext<'_> {
    ToolContext {
        skills,
        notes: None,
    }
}

async fn call(registry: &ToolRegistry, reply: &str) -> ToolOutcome {
    let parsed = registry.parse(reply).expect("a registered call");
    let skills = SkillCatalogue::default();
    parsed.tool.run(&context(&skills), parsed.args).await
}

#[test]
fn a_registered_tool_is_described_in_the_generated_prompt() {
    let prompt = registry().prompt_section();
    assert!(prompt.contains("- echo repeats text."));
    assert!(prompt.contains("text (string), times (integer, optional)"));
    assert!(prompt.contains(r#"Example: {"tool":"echo","text":"..."}"#));
}

#[test]
fn an_empty_registry_adds_nothing_to_the_prompt() {
    assert_eq!(ToolRegistry::default().prompt_section(), "");
}

#[tokio::test]
async fn a_text_protocol_call_runs_the_tool() {
    let outcome = call(&registry(), r#"TOOL {"tool":"echo","text":"ab","times":2}"#).await;
    assert_eq!(outcome, ToolOutcome::output("Echoed", "abab"));
}

#[tokio::test]
async fn name_and_arguments_and_code_fences_are_tolerated() {
    let reply = "```json\n{\"name\":\"echo\",\"arguments\":{\"text\":\"x\"}}\n```";
    assert_eq!(
        call(&registry(), reply).await,
        ToolOutcome::output("Echoed", "x")
    );
}

#[test]
fn an_unregistered_tool_is_not_a_call() {
    assert!(
        registry()
            .parse(r#"TOOL {"tool":"rm","path":"/"}"#)
            .is_none()
    );
    assert!(registry().parse("no call here").is_none());
}

#[tokio::test]
async fn invalid_arguments_become_an_error_for_the_model() {
    let outcome = call(&registry(), r#"TOOL {"tool":"echo","times":"many"}"#).await;
    let ToolOutcome { text, .. } = outcome;
    assert!(text.starts_with("Error: invalid arguments for echo"));
    assert!(text.contains("text (string)"));
}
