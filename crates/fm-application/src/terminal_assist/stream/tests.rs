use super::LineStreamer;

fn collect(
    run: impl FnOnce(&mut LineStreamer, &mut dyn FnMut(&str)),
) -> (LineStreamer, Vec<String>) {
    let mut streamer = LineStreamer::new();
    let mut lines = Vec::new();
    run(&mut streamer, &mut |line| lines.push(line.to_owned()));
    (streamer, lines)
}

#[test]
fn complete_lines_are_emitted_as_they_arrive_and_the_rest_at_the_end() {
    let (_, lines) = collect(|streamer, emit| {
        streamer.push("Hel", emit);
        streamer.push("lo\r\nWor", emit);
        streamer.push("ld\n\nEnd", emit);
        streamer.finish(emit);
    });
    assert_eq!(lines, ["Hello", "World", "", "End"]);
}

#[test]
fn a_tool_call_line_and_what_follows_it_are_held_back() {
    let (mut streamer, lines) = collect(|streamer, emit| {
        streamer.push("Let me look.\nTOOL {\"tool\":\"list_dir\"}\nmore\n", emit);
        streamer.finish(emit);
    });
    assert_eq!(lines, ["Let me look."]);
    let mut released = Vec::new();
    streamer.release_held(&mut |line| released.push(line.to_owned()));
    assert_eq!(released, ["TOOL {\"tool\":\"list_dir\"}", "more"]);
    streamer.release_held(&mut |line| released.push(line.to_owned()));
    assert_eq!(released.len(), 2);
}

#[test]
fn xml_and_json_shaped_lines_are_held_until_it_is_known_they_are_not_tool_calls() {
    for held in [
        "<tool_call> {\"tool\": \"run_command\"}",
        "{\"name\": \"read_file\", \"arguments\": {}}",
        "```json",
    ] {
        let (_, lines) = collect(|streamer, emit| {
            streamer.push(&format!("{held}\n"), emit);
            streamer.finish(emit);
        });
        assert!(lines.is_empty(), "{held}");
    }
    let (_, lines) = collect(|streamer, emit| {
        streamer.push("{\"a\": 1} is plain\n", emit);
        streamer.finish(emit);
    });
    assert_eq!(lines, ["{\"a\": 1} is plain"]);
}

#[test]
fn discarding_drops_the_held_lines() {
    let (mut streamer, _) = collect(|streamer, emit| {
        streamer.push("TOOL {\"tool\":\"grep\"}\n", emit);
    });
    streamer.discard_held();
    let mut released = Vec::new();
    streamer.release_held(&mut |line| released.push(line.to_owned()));
    assert!(released.is_empty());
}
