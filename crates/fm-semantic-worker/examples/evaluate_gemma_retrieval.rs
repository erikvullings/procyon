//! Local, non-production retrieval check of EmbeddingGemma 2 over chunk dumps.
//!
//! Companion of `scripts/eval/retrieval/retrieve.py` (which runs e5-small): reads the same
//! `queries.json` and `<chunks>/<maximum>/<pdf>.jsonl` dumps, embeds queries as `search` and
//! chunks as `document`, and prints hit@k and MRR per candidate maximum.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use fm_semantic_worker::gemma_probe::{GemmaTextProbe, GemmaTextTask};
use serde::Deserialize;

#[derive(Deserialize)]
struct Query {
    q: String,
    doc: String,
    evidence: String,
}

#[derive(Deserialize)]
struct Chunk {
    doc: String,
    text: String,
}

fn normalise(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let [model, chunks, queries] = arguments.as_slice() else {
        return Err("usage: evaluate_gemma_retrieval MODEL_DIR CHUNKS_DIR QUERIES_JSON [env GEMMA_MAXIMA=512,1000]".into());
    };
    let wanted: Option<HashSet<String>> = env::var("GEMMA_MAXIMA")
        .ok()
        .map(|value| value.split(',').map(str::to_owned).collect());
    let probe = GemmaTextProbe::open(Path::new(model), 768)?;
    let queries: Vec<Query> = serde_json::from_str(&fs::read_to_string(queries)?)?;
    let query_vectors = queries
        .iter()
        .map(|query| probe.encode(GemmaTextTask::Search, &query.q, None))
        .collect::<Result<Vec<_>, _>>()?;
    let docs: Vec<&str> = queries.iter().map(|query| query.doc.as_str()).collect();

    let mut directories: Vec<PathBuf> = fs::read_dir(chunks)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            wanted.as_ref().is_none_or(|set| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| set.contains(name))
            })
        })
        .collect();
    directories.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u32>().ok())
    });

    println!("| max (est. tokens) | chunks | answerable | hit@1 | hit@3 | hit@5 | MRR |");
    println!("|---|---|---|---|---|---|---|");
    for directory in directories {
        let mut pool: Vec<Chunk> = Vec::new();
        let mut files: Vec<PathBuf> = fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        files.sort();
        for file in files {
            for line in fs::read_to_string(file)?.lines() {
                let chunk: Chunk = serde_json::from_str(line)?;
                // Only documents that have questions: the rest are shared by every candidate.
                if docs.iter().any(|doc| chunk.doc.starts_with(doc)) {
                    pool.push(chunk);
                }
            }
        }
        let vectors = pool
            .iter()
            .map(|chunk| probe.encode(GemmaTextTask::Document, &chunk.text, None))
            .collect::<Result<Vec<_>, _>>()?;
        let (mut answerable, mut reciprocal) = (0_usize, 0.0_f64);
        let mut hits = [0_usize; 3];
        for (query, vector) in queries.iter().zip(&query_vectors) {
            let evidence = normalise(&query.evidence);
            let relevant: HashSet<usize> = pool
                .iter()
                .enumerate()
                .filter(|(_, chunk)| {
                    chunk.doc.starts_with(&query.doc) && normalise(&chunk.text).contains(&evidence)
                })
                .map(|(index, _)| index)
                .collect();
            if relevant.is_empty() {
                continue;
            }
            answerable += 1;
            let mut order: Vec<usize> = (0..pool.len()).collect();
            order.sort_by(|&a, &b| dot(vector, &vectors[b]).total_cmp(&dot(vector, &vectors[a])));
            let rank = order
                .iter()
                .position(|index| relevant.contains(index))
                .map_or(usize::MAX, |at| at + 1);
            reciprocal += 1.0 / rank as f64;
            for (slot, k) in hits.iter_mut().zip([1, 3, 5]) {
                *slot += usize::from(rank <= k);
            }
        }
        let n = answerable.max(1) as f64;
        println!(
            "| {} | {} | {}/{} | {:.2} | {:.2} | {:.2} | {:.3} |",
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?"),
            pool.len(),
            answerable,
            queries.len(),
            hits[0] as f64 / n,
            hits[1] as f64 / n,
            hits[2] as f64 / n,
            reciprocal / n,
        );
    }
    Ok(())
}
