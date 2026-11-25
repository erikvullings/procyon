use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::*;

#[derive(Deserialize, JsonSchema)]
struct EchoArgs {
    /// The text to repeat.
    text: String,
    /// How often.
    times: Option<u32>,
}

struct Echo;

#[async_trait]
impl<I: Send + 'static> TypedTool<I> for Echo {
    type Args = EchoArgs;

    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Repeats text."
    }

    async fn run_typed(&self, _ctx: &ToolContext<'_>, args: EchoArgs) -> ToolOutcome<I> {
        ToolOutcome::output("Echoed", args.text.repeat(args.times.unwrap_or(1) as usize))
    }
}

/// The host interrupts of these tests.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Interrupt {
    Command(CommandRequest),
    Note(NoteProposal),
}

impl From<CommandRequest> for Interrupt {
    fn from(request: CommandRequest) -> Self {
        Self::Command(request)
    }
}

impl From<NoteProposal> for Interrupt {
    fn from(proposal: NoteProposal) -> Self {
        Self::Note(proposal)
    }
}

fn registry() -> ToolRegistry<Interrupt> {
    let mut registry = ToolRegistry::new();
    registry.register(Typed(Echo));
    registry
}

async fn call(registry: &ToolRegistry<Interrupt>, reply: &str) -> ToolOutcome<Interrupt> {
    let mut parsed = registry.parse(reply, 8);
    assert_eq!(parsed.len(), 1, "{reply}");
    let parsed = parsed.remove(0);
    let skills = SkillCatalogue::default();
    parsed
        .tool
        .run(&ToolContext::new(&skills), parsed.args)
        .await
}

#[test]
fn a_registered_tool_is_described_in_the_generated_prompt_and_specs() {
    let registry = registry();
    let prompt = registry.prompt_section();
    assert!(prompt.contains("TOOL {json}"));
    assert!(prompt.contains("- echo: Repeats text."));
    assert!(prompt.contains("text (string), times (integer, optional)"));
    let example = prompt.split("Example: TOOL ").nth(1).expect("an example");
    let example: Value = serde_json::from_str(example).expect("the example is JSON");
    assert_eq!(example, json!({"tool": "echo", "text": "..."}));

    let specs = registry.specs();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "echo");
    assert_eq!(
        specs[0].schema["properties"]["text"]["description"],
        "The text to repeat."
    );
    assert_eq!(specs[0].schema["required"], json!(["text"]));
}

#[test]
fn derived_schemas_are_plain_objects_endpoints_accept() {
    let schema = schema_for::<EchoArgs>();
    assert_eq!(schema["type"], "object");
    assert!(schema.get("$schema").is_none());
    assert!(schema.get("title").is_none());
    // `Option<u32>` is optional through `required`, not a `["integer", "null"]` type.
    assert_eq!(schema["properties"]["times"]["type"], "integer");
    assert!(schema["properties"]["times"].get("format").is_none());
}

#[test]
fn the_tool_section_depends_on_how_tools_are_called() {
    let registry = registry();
    assert_eq!(registry.prompt_for(ToolCalling::Native), "");
    assert!(registry.specs_for(ToolCalling::Text).is_empty());
    assert_eq!(registry.specs_for(ToolCalling::Native).len(), 1);
    let prompt = compose_system_prompt(
        "Base.\n",
        &registry,
        ToolCalling::Text,
        &SkillCatalogue::default(),
        "",
    );
    assert!(prompt.starts_with("Base.\n\nTo use a tool"));
    let native = compose_system_prompt(
        "Base.",
        &registry,
        ToolCalling::Native,
        &SkillCatalogue::default(),
        "",
    );
    assert_eq!(native, "Base.");
}

#[test]
fn a_host_can_describe_a_tool_in_its_own_words() {
    let mut registry = registry();
    assert!(registry.describe("echo", "Repeats text, loudly."));
    assert!(!registry.describe("missing", "x"));
    assert!(
        registry
            .prompt_section()
            .contains("- echo: Repeats text, loudly.")
    );
    assert_eq!(registry.specs()[0].description, "Repeats text, loudly.");
}

#[test]
fn an_empty_registry_adds_nothing_to_the_prompt() {
    assert_eq!(ToolRegistry::<Interrupt>::new().prompt_section(), "");
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
            .parse(r#"TOOL {"tool":"rm","path":"/"}"#, 8)
            .is_empty()
    );
    assert!(registry().parse("no call here", 8).is_empty());
}

#[tokio::test]
async fn invalid_arguments_become_an_error_for_the_model() {
    let text = call(&registry(), r#"TOOL {"tool":"echo","times":"many"}"#)
        .await
        .expect_text();
    assert!(text.starts_with("Error: invalid arguments for echo"));
    assert!(text.contains("text (string)"));
}

async fn core(reply: &str) -> ToolOutcome<Interrupt> {
    call(&command_registry(), reply).await
}

#[tokio::test]
async fn the_built_in_tools_check_their_arguments_before_any_access() {
    for (reply, message) in [
        (
            r#"TOOL {"tool":"search_notes","query":"  "}"#,
            "query must not be empty",
        ),
        (
            r#"TOOL {"tool":"propose_note","content":""}"#,
            "content must not be empty",
        ),
        (
            r#"TOOL {"tool":"propose_note_update","path":" ","content":"x"}"#,
            "path must not be empty",
        ),
        (
            r#"TOOL {"tool":"read_file","path":""}"#,
            "path must not be empty",
        ),
        (
            r#"TOOL {"tool":"grep","pattern":""}"#,
            "pattern must not be empty",
        ),
        (
            r#"TOOL {"tool":"run_command","command":"  ","explanation":"x"}"#,
            "one non-empty line",
        ),
        (
            r#"TOOL {"tool":"run_command","command":"ls\nrm -rf x","explanation":"x"}"#,
            "one non-empty line",
        ),
        (
            r#"TOOL {"tool":"run_command","command":"ls","explanation":" "}"#,
            "give `explanation`",
        ),
        (
            r#"TOOL {"tool":"run_command","command":"ls"}"#,
            "invalid arguments for run_command",
        ),
        (
            r#"TOOL {"tool":"read_file"}"#,
            "invalid arguments for read_file",
        ),
    ] {
        let text = core(reply).await.expect_text();
        assert!(text.contains(message), "{reply}: {text}");
    }
}

#[tokio::test]
async fn valid_calls_without_access_are_explained_not_dropped() {
    let text = core(r#"TOOL {"tool":"search_notes","query":"tls","limit":3}"#)
        .await
        .expect_text();
    assert_eq!(text, "Error: notes are not available here.");
    let text = core(r#"TOOL {"tool":"list_dir","path":"."}"#)
        .await
        .expect_text();
    assert_eq!(text, "Error: file tools are not available here.");
}

#[tokio::test]
async fn run_command_interrupts_with_the_trimmed_line() {
    assert_eq!(
        core(
            r#"TOOL {"tool":"run_command","command":" cargo test ","explanation":" Runs the tests. "}"#
        )
        .await,
        ToolOutcome::Interrupt(Interrupt::Command(CommandRequest {
            command: "cargo test".into(),
            explanation: "Runs the tests.".into(),
        }))
    );
}

#[test]
fn the_registries_offer_the_documented_tools_with_their_effects() {
    let registry = command_registry::<Interrupt>();
    let names: Vec<&str> = registry.names().collect();
    assert_eq!(
        names,
        [
            "grep",
            "list_dir",
            "propose_note",
            "propose_note_update",
            "read_file",
            "read_note",
            "read_skill",
            "read_skill_file",
            "run_command",
            "search_notes",
            "summarize_file",
        ]
    );
    let read_only = read_only_registry::<Interrupt>();
    assert!(!read_only.contains("run_command"));
    for call in read_only.parse(
        "TOOL {\"tool\":\"list_dir\",\"path\":\".\"}\nTOOL {\"tool\":\"read_skill\",\"name\":\"x\"}",
        8,
    ) {
        assert_eq!(call.tool.effect(), ToolEffect::ReadOnly);
    }
    for call in registry.parse(
        "TOOL {\"tool\":\"run_command\",\"command\":\"ls\"}\nTOOL {\"tool\":\"propose_note\",\"content\":\"x\"}",
        8,
    ) {
        assert_eq!(call.tool.effect(), ToolEffect::Exclusive);
    }
}

#[test]
fn every_built_in_schema_matches_its_arguments() {
    for spec in command_registry::<Interrupt>().specs() {
        assert_eq!(spec.schema["type"], "object", "{}", spec.name);
        let required = spec.schema["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let properties = spec.schema["properties"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for name in &required {
            assert!(
                properties.contains_key(name.as_str().unwrap_or_default()),
                "{}: {name}",
                spec.name
            );
        }
        for (name, property) in &properties {
            assert!(
                property.get("description").is_some(),
                "{}.{name} has no description for the model",
                spec.name
            );
        }
    }
}
