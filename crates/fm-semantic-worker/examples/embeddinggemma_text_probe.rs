//! Local, non-production EmbeddingGemma 2 CPU text parity probe.

use std::env;
use std::io;
use std::path::Path;

use fm_semantic_worker::gemma_probe::{GemmaTextProbe, GemmaTextTask};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if !(4..=5).contains(&arguments.len()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: embeddinggemma_text_probe MODEL_DIR DIMENSION search|question|code|document TEXT [TITLE]",
        )
        .into());
    }
    let dimensions: usize = arguments[1].parse()?;
    let task = match arguments[2].as_str() {
        "search" => GemmaTextTask::Search,
        "question" => GemmaTextTask::Question,
        "code" => GemmaTextTask::Code,
        "document" => GemmaTextTask::Document,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown text task: {other}"),
            )
            .into());
        }
    };
    let probe = GemmaTextProbe::open(Path::new(&arguments[0]), dimensions)?;
    let embedding = probe.encode(task, &arguments[3], arguments.get(4).map(String::as_str))?;
    println!("{}", serde_json::to_string(&embedding)?);
    Ok(())
}
