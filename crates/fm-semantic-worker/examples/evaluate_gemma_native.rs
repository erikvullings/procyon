//! Opt-in, model-level CPU retrieval evidence from the pinned native checkpoint.

use std::collections::HashSet;
use std::error::Error;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Instant;

use fm_semantic_worker::gemma_native::{GemmaMedia, GemmaNativeEncoder};
use fm_semantic_worker::gemma_probe::GemmaTextTask;
use image::{ImageBuffer, ImageFormat, Rgb};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const REVISION: &str = "914f7f89142e33e77833254d9c9b90c3cef7303b";
const KNOWLEDGE: &str =
    include_str!("../../fm-application/tests/fixtures/knowledge-retrieval-corpus-v1.json");
const CODE: &str = include_str!("../../../scripts/fixtures/embedding-code-search-v1.json");
const DIMENSIONS: [usize; 4] = [128, 256, 512, 768];

#[derive(Deserialize)]
struct KnowledgeCorpus {
    #[serde(rename = "authorizedRootIds")]
    authorized_roots: Vec<String>,
    documents: Vec<KnowledgeDocument>,
    cases: Vec<KnowledgeCase>,
}

#[derive(Deserialize)]
struct KnowledgeDocument {
    occurrences: Vec<Occurrence>,
    chunks: Vec<Chunk>,
}

#[derive(Deserialize)]
struct Occurrence {
    #[serde(rename = "sourceId")]
    source_id: String,
    #[serde(rename = "rootId")]
    root_id: String,
    available: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(rename = "sectionPath")]
    sections: Vec<String>,
    text: String,
}

#[derive(Deserialize)]
struct KnowledgeCase {
    id: String,
    question: String,
    #[serde(rename = "relevantSourceIds")]
    relevant: Vec<String>,
    #[serde(rename = "expectedNoEvidence")]
    negative: bool,
}

#[derive(Deserialize)]
struct CodeCorpus {
    documents: Vec<CodeDocument>,
    cases: Vec<CodeCase>,
}

#[derive(Deserialize)]
struct CodeDocument {
    id: String,
    text: String,
}

#[derive(Deserialize)]
struct CodeCase {
    id: String,
    query: String,
    #[serde(rename = "relevantIds")]
    relevant: Vec<String>,
}

struct Document {
    ids: Vec<String>,
    text: String,
}

struct ImageSubject {
    id: String,
    png: Vec<u8>,
}

struct Query {
    id: String,
    text: String,
    relevant: Vec<String>,
    task: GemmaTextTask,
    eligible: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct LabelledCorpus {
    documents: Vec<CodeDocument>,
    cases: Vec<LabelledCase>,
}

#[derive(Deserialize)]
struct LabelledCase {
    id: String,
    query: String,
    #[serde(rename = "relevantIds")]
    relevant: Vec<String>,
    #[serde(rename = "eligibleIds")]
    eligible: Vec<String>,
}

#[derive(Deserialize)]
struct LabelledPhoto {
    id: String,
    path: String,
    sha256: String,
    #[serde(rename = "positiveCaption")]
    positive: String,
    #[serde(rename = "negativeCaption")]
    negative: String,
}

fn verify_checkpoint(directory: &Path) -> Result<(), Box<dyn Error>> {
    for (name, bytes, expected) in [
        (
            "model.safetensors",
            1_488_915_288,
            "197a32965d4b1105faf060417baa899e193fb73cd401f42ec9295234d5553d79",
        ),
        (
            "tokenizer.json",
            32_170_510,
            "4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4",
        ),
        (
            "config.json",
            4_455,
            "b8f1e9931b57fbc054acdb445c41765d55b0074c58d145fa82839941ad1b5bb3",
        ),
        (
            "processor_config.json",
            1_788,
            "168f6a08522f3ce5dea596d94d003af2fd691742d4f41fe1f9d8cce76bfbf69c",
        ),
        (
            "preprocessor_config.json",
            511,
            "ea2ae257e901064abdd98dceb19f2b0da06af600bed15e0f99f5c85c37ee9d78",
        ),
    ] {
        let path = directory.join(name);
        if path.metadata()?.len() != bytes {
            return Err(format!("{name} is not the pinned checkpoint size").into());
        }
        let mut file = File::open(path)?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        if hex_digest(digest.finalize().as_ref()) != expected {
            return Err(format!("{name} is not the pinned checkpoint digest").into());
        }
    }
    Ok(())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn normalized_score(left: &[f32], right: &[f32], dimensions: usize) -> f64 {
    let (mut product, mut left_norm, mut right_norm) = (0.0, 0.0, 0.0);
    for (&a, &b) in left[..dimensions].iter().zip(&right[..dimensions]) {
        let (a, b) = (f64::from(a), f64::from(b));
        product += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    product / (left_norm * right_norm).sqrt()
}

fn rank(
    documents: &[Document],
    vectors: &[Vec<f32>],
    query: &[f32],
    dimensions: usize,
    eligible: Option<&HashSet<&str>>,
) -> Vec<String> {
    let mut indices = (0..documents.len()).collect::<Vec<_>>();
    indices.sort_by(|&left, &right| {
        normalized_score(&vectors[right], query, dimensions)
            .total_cmp(&normalized_score(&vectors[left], query, dimensions))
            .then_with(|| left.cmp(&right))
    });
    let mut seen = HashSet::new();
    indices
        .into_iter()
        .flat_map(|index| documents[index].ids.iter().cloned())
        .filter(|id| eligible.is_none_or(|allowed| allowed.contains(id.as_str())))
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn metrics(
    documents: &[Document],
    vectors: &[Vec<f32>],
    queries: &[Query],
    query_vectors: &[Vec<f32>],
    dimensions: usize,
) -> Result<Value, Box<dyn Error>> {
    if documents.len() != vectors.len()
        || queries.len() != query_vectors.len()
        || documents.is_empty()
        || queries.is_empty()
        || vectors.iter().chain(query_vectors).any(|vector| {
            vector.len() < dimensions
                || vector[..dimensions].iter().any(|value| !value.is_finite())
                || vector[..dimensions].iter().all(|value| *value == 0.0)
        })
    {
        return Err("benchmark vectors are missing or invalid".into());
    }
    let eligible: HashSet<&str> = documents
        .iter()
        .flat_map(|document| document.ids.iter().map(String::as_str))
        .collect();
    let mut cases = Vec::new();
    let (mut hits_at_one, mut hits_at_three, mut reciprocal_rank) = (0.0, 0.0, 0.0);
    let mut recall_at_two = 0.0;
    let mut positives = 0;
    for (case, query_vector) in queries.iter().zip(query_vectors) {
        let allowed = case
            .eligible
            .as_ref()
            .map(|ids| ids.iter().map(String::as_str).collect::<HashSet<_>>());
        if allowed
            .as_ref()
            .is_some_and(|ids| ids.is_empty() || ids.iter().any(|id| !eligible.contains(id)))
            || case.relevant.iter().any(|id| {
                allowed
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(id.as_str()))
            })
        {
            return Err(format!("{} has inconsistent eligible labels", case.id).into());
        }
        let ranked = rank(
            documents,
            vectors,
            query_vector,
            dimensions,
            allowed.as_ref(),
        );
        if case.relevant.is_empty() {
            cases.push(json!({"id": case.id, "negativeControl": true, "topIds": &ranked[..ranked.len().min(3)]}));
            continue;
        }
        if case
            .relevant
            .iter()
            .any(|id| !eligible.contains(id.as_str()))
        {
            return Err(format!("{} references an ineligible labelled document", case.id).into());
        }
        positives += 1;
        let relevant: HashSet<&str> = case.relevant.iter().map(String::as_str).collect();
        let first = ranked.iter().position(|id| relevant.contains(id.as_str()));
        let hit_one = f64::from(first == Some(0));
        let hit_three = f64::from(first.is_some_and(|position| position < 3));
        let rr = first.map_or(0.0, |position| 1.0 / (position + 1) as f64);
        hits_at_one += hit_one;
        hits_at_three += hit_three;
        reciprocal_rank += rr;
        let at_two = ranked[..ranked.len().min(2)]
            .iter()
            .filter(|id| relevant.contains(id.as_str()))
            .count() as f64
            / relevant.len() as f64;
        recall_at_two += at_two;
        cases.push(json!({
            "id": case.id, "firstRelevantRank": first.map(|position| position + 1),
            "recallAt2": at_two, "topIds": &ranked[..ranked.len().min(3)]
        }));
    }
    if positives == 0 {
        return Err("benchmark has no positive labels".into());
    }
    Ok(json!({
        "positiveCases": positives,
        "negativeControlsWithoutThreshold": queries.len() - positives,
        "hitAt1": hits_at_one / positives as f64,
        "hitAt3": hits_at_three / positives as f64,
        "recallAt2": recall_at_two / positives as f64,
        "mrr": reciprocal_rank / positives as f64,
        "cases": cases
    }))
}

fn image_fixture() -> Result<Vec<ImageSubject>, Box<dyn Error>> {
    let subjects = [
        ("red-circle", [229_u8, 43, 50], "circle"),
        ("blue-square", [23, 80, 219], "square"),
        ("green-triangle", [26, 164, 91], "triangle"),
        ("yellow-circle", [224, 187, 25], "circle"),
        ("red-square", [229, 43, 50], "square"),
        ("blue-circle", [23, 80, 219], "circle"),
        ("green-circle", [26, 164, 91], "circle"),
        ("yellow-square", [224, 187, 25], "square"),
    ];
    subjects
        .into_iter()
        .map(|(name, color, shape)| {
            let image = ImageBuffer::from_fn(128, 128, |x, y| {
                let (dx, dy) = (x as i32 - 64, y as i32 - 64);
                let filled = match shape {
                    "circle" => dx * dx + dy * dy <= 38 * 38,
                    "square" => dx.abs() <= 38 && dy.abs() <= 38,
                    "triangle" => (-35..=38).contains(&dy) && dx.abs() <= (38 - dy) / 2,
                    _ => unreachable!(),
                };
                Rgb(if filled { color } else { [255, 255, 255] })
            });
            let mut encoded = Cursor::new(Vec::new());
            image.write_to(&mut encoded, ImageFormat::Png)?;
            Ok(ImageSubject {
                id: name.to_owned(),
                png: encoded.into_inner(),
            })
        })
        .collect()
}

fn evaluate_images(encoder: &GemmaNativeEncoder) -> Result<Value, Box<dyn Error>> {
    let fixture = image_fixture()?;
    let documents = fixture
        .iter()
        .map(|subject| Document {
            ids: vec![subject.id.clone()],
            text: String::new(),
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    let vectors = fixture
        .iter()
        .map(|subject| encoder.encode_image(&subject.png))
        .collect::<Result<Vec<_>, _>>()?;
    let image_seconds = started.elapsed().as_secs_f64();
    let queries = [
        ("red-circle", "red circle"),
        ("blue-square", "blue square"),
        ("green-triangle", "green triangle"),
        ("yellow-circle", "yellow circle"),
    ]
    .into_iter()
    .map(|(id, text)| Query {
        id: id.to_owned(),
        text: format!("a {text} on a white background"),
        relevant: vec![id.to_owned()],
        task: GemmaTextTask::Search,
        eligible: None,
    })
    .collect::<Vec<_>>();
    let mut query_latencies = Vec::new();
    let mut query_vectors = Vec::new();
    for query in &queries {
        let started = Instant::now();
        query_vectors.push(encoder.encode_text(query.task, &query.text, None)?);
        query_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    query_latencies.sort_by(f64::total_cmp);
    let mut by_dimension = serde_json::Map::new();
    for dimensions in DIMENSIONS {
        by_dimension.insert(
            dimensions.to_string(),
            metrics(&documents, &vectors, &queries, &query_vectors, dimensions)?,
        );
    }
    Ok(json!({
        "fixture": "eight generated PNG color/shape subjects; distractors share color or shape",
        "imageCount": documents.len(),
        "imagesPerSecond": documents.len() as f64 / image_seconds,
        "queryLatencyMedianMs": median(&query_latencies),
        "dimensions": by_dimension
    }))
}

fn evaluate(
    encoder: &GemmaNativeEncoder,
    documents: Vec<Document>,
    queries: Vec<Query>,
    corpus_bytes: &str,
) -> Result<Value, Box<dyn Error>> {
    let started = Instant::now();
    let vectors = documents
        .iter()
        .map(|document| encoder.encode_text(GemmaTextTask::Document, &document.text, None))
        .collect::<Result<Vec<_>, _>>()?;
    let document_seconds = started.elapsed().as_secs_f64();
    let mut query_latencies = Vec::new();
    let mut query_vectors = Vec::new();
    for query in &queries {
        let started = Instant::now();
        query_vectors.push(encoder.encode_text(query.task, &query.text, None)?);
        query_latencies.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    query_latencies.sort_by(f64::total_cmp);
    let mut by_dimension = serde_json::Map::new();
    for dimensions in DIMENSIONS {
        by_dimension.insert(
            dimensions.to_string(),
            metrics(&documents, &vectors, &queries, &query_vectors, dimensions)?,
        );
    }
    Ok(json!({
        "corpusSha256": hex_digest(Sha256::digest(corpus_bytes.as_bytes()).as_ref()),
        "documentCount": documents.len(),
        "queryCount": queries.len(),
        "documentSeconds": document_seconds,
        "documentsPerSecond": documents.len() as f64 / document_seconds,
        "queryLatencyMedianMs": median(&query_latencies),
        "queryLatencyP95Ms": query_latencies[(query_latencies.len() * 95).div_ceil(100) - 1],
        "dimensions": by_dimension
    }))
}

fn evaluate_labelled(
    encoder: &GemmaNativeEncoder,
    path: &Path,
    task: GemmaTextTask,
) -> Result<Value, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let corpus: LabelledCorpus = serde_json::from_str(&raw)?;
    if corpus.cases.iter().any(|case| case.relevant.is_empty()) {
        return Err("labelled evaluation has a case without a judged positive".into());
    }
    let documents = corpus
        .documents
        .into_iter()
        .map(|document| Document {
            ids: vec![document.id],
            text: document.text,
        })
        .collect();
    let queries = corpus
        .cases
        .into_iter()
        .map(|case| Query {
            id: case.id,
            text: case.query,
            relevant: case.relevant,
            task,
            eligible: Some(case.eligible),
        })
        .collect();
    evaluate(encoder, documents, queries, &raw)
}

fn evaluate_labelled_photos(
    encoder: &GemmaNativeEncoder,
    path: &Path,
) -> Result<Value, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let photos: Vec<LabelledPhoto> = serde_json::from_str(&raw)?;
    if photos.is_empty() {
        return Err("photo fixture is empty".into());
    }
    let mut by_dimension = serde_json::Map::new();
    let mut scored = DIMENSIONS
        .into_iter()
        .map(|dimension| (dimension, Vec::new()))
        .collect::<std::collections::HashMap<_, _>>();
    for photo in &photos {
        let bytes = fs::read(&photo.path)?;
        if hex_digest(Sha256::digest(&bytes).as_ref()) != photo.sha256 {
            return Err(format!("photo {} differs from its pinned digest", photo.id).into());
        }
        let image = encoder.encode_image(&bytes)?;
        let positive = encoder.encode_text(GemmaTextTask::Search, &photo.positive, None)?;
        let negative = encoder.encode_text(GemmaTextTask::Search, &photo.negative, None)?;
        for dimension in DIMENSIONS {
            let margin = normalized_score(&image, &positive, dimension)
                - normalized_score(&image, &negative, dimension);
            if !margin.is_finite() {
                return Err(format!("photo {} produced invalid similarity", photo.id).into());
            }
            scored
                .get_mut(&dimension)
                .ok_or("missing dimension")?
                .push(json!({
                    "id": photo.id, "positiveBeatsHardNegative": margin > 0.0,
                    "cosineMargin": margin
                }));
        }
    }
    for dimension in DIMENSIONS {
        let cases = scored.remove(&dimension).ok_or("missing dimension")?;
        let correct = cases
            .iter()
            .filter(|case| case["positiveBeatsHardNegative"] == true)
            .count();
        by_dimension.insert(
            dimension.to_string(),
            json!({"accuracy": correct as f64 / cases.len() as f64, "cases": cases}),
        );
    }
    Ok(json!({
        "fixtureSha256": hex_digest(Sha256::digest(raw.as_bytes()).as_ref()),
        "pairCount": photos.len(),
        "task": "real-photo image versus independently validated positive/hard-negative captions",
        "dimensions": by_dimension
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let directory = args
        .first()
        .ok_or("usage: evaluate_gemma_native MODEL_DIR [--labelled-corpus JSON search|code | --labelled-photos JSON]")?;
    let directory = Path::new(&directory);
    verify_checkpoint(directory)?;
    let started = Instant::now();
    let encoder = GemmaNativeEncoder::open(
        directory,
        768,
        GemmaMedia {
            images: true,
            audio: false,
            video: false,
        },
    )?;
    let load_seconds = started.elapsed().as_secs_f64();
    if args.len() != 1 {
        let result = match args.get(1).and_then(|value| value.to_str()) {
            Some("--labelled-corpus") if args.len() == 4 => {
                let task = match args[3].to_str() {
                    Some("search") => GemmaTextTask::Search,
                    Some("code") => GemmaTextTask::Code,
                    _ => return Err("labelled corpus intent must be search or code".into()),
                };
                evaluate_labelled(&encoder, Path::new(&args[2]), task)?
            }
            Some("--labelled-photos") if args.len() == 3 => {
                evaluate_labelled_photos(&encoder, Path::new(&args[2]))?
            }
            _ => return Err("invalid labelled evaluation arguments".into()),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "model": "google/embeddinggemma-2", "revision": REVISION,
                "runtime": "native-rust-cpu-fp32", "measurementWidth": 768,
                "dimensionMethod": "truncate and L2 renormalize both sides",
                "os": std::env::consts::OS, "architecture": std::env::consts::ARCH,
                "modelLoadSeconds": load_seconds, "result": result
            }))?
        );
        return Ok(());
    }

    let knowledge: KnowledgeCorpus = serde_json::from_str(KNOWLEDGE)?;
    let documents = knowledge
        .documents
        .into_iter()
        .filter_map(|document| {
            let ids = document
                .occurrences
                .into_iter()
                .filter(|occurrence| {
                    occurrence.available && knowledge.authorized_roots.contains(&occurrence.root_id)
                })
                .map(|occurrence| occurrence.source_id)
                .collect::<Vec<_>>();
            (!ids.is_empty()).then_some(
                document
                    .chunks
                    .into_iter()
                    .map(|chunk| Document {
                        ids: ids.clone(),
                        text: format!("{}\n{}", chunk.sections.join(" > "), chunk.text),
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect();
    let queries = knowledge
        .cases
        .into_iter()
        .map(|case| {
            if case.negative != case.relevant.is_empty() {
                return Err(format!("{} has inconsistent negative label", case.id));
            }
            Ok(Query {
                id: case.id,
                text: case.question,
                relevant: case.relevant,
                task: GemmaTextTask::Search,
                eligible: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let knowledge_report = evaluate(&encoder, documents, queries, KNOWLEDGE)?;

    let code: CodeCorpus = serde_json::from_str(CODE)?;
    let documents = code
        .documents
        .into_iter()
        .map(|document| Document {
            ids: vec![document.id],
            text: document.text,
        })
        .collect();
    let queries = code
        .cases
        .into_iter()
        .map(|case| {
            if case.relevant.is_empty() {
                return Err(format!("{} has no code label", case.id));
            }
            Ok(Query {
                id: case.id,
                text: case.query,
                relevant: case.relevant,
                task: GemmaTextTask::Code,
                eligible: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let code_report = evaluate(&encoder, documents, queries, CODE)?;
    let image_report = evaluate_images(&encoder)?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "model": "google/embeddinggemma-2",
            "revision": REVISION,
            "runtime": "native-rust-cpu-fp32",
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "modelLoadSeconds": load_seconds,
            "measurementWidth": 768,
            "dimensionMethod": "truncate and L2 renormalize both sides",
            "knowledge": knowledge_report,
            "code": code_report,
            "images": image_report
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_excludes_negative_controls_and_checks_label_eligibility() {
        let documents = vec![
            Document {
                ids: vec!["correct".into()],
                text: String::new(),
            },
            Document {
                ids: vec!["distractor".into()],
                text: String::new(),
            },
        ];
        let vectors = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let queries = vec![
            Query {
                id: "positive".into(),
                text: String::new(),
                relevant: vec!["correct".into()],
                task: GemmaTextTask::Search,
                eligible: None,
            },
            Query {
                id: "negative".into(),
                text: String::new(),
                relevant: vec![],
                task: GemmaTextTask::Search,
                eligible: None,
            },
        ];
        let result = metrics(&documents, &vectors, &queries, &vectors, 2).unwrap();
        assert_eq!(result["positiveCases"], 1);
        assert_eq!(result["negativeControlsWithoutThreshold"], 1);
        assert_eq!(result["hitAt1"], 1.0);
        assert_eq!(result["mrr"], 1.0);
        assert_eq!(result["cases"][1]["negativeControl"], true);
        let invalid = Query {
            id: "invalid".into(),
            text: String::new(),
            relevant: vec!["missing".into()],
            task: GemmaTextTask::Search,
            eligible: None,
        };
        assert!(metrics(&documents, &vectors, &[invalid], &vectors, 2).is_err());
        assert!(metrics(&documents, &vectors[..1], &queries, &vectors, 2).is_err());
    }

    #[test]
    fn ranking_only_uses_assessed_candidates() {
        let documents = ["positive", "unjudged", "negative"]
            .into_iter()
            .map(|id| Document {
                ids: vec![id.into()],
                text: String::new(),
            })
            .collect::<Vec<_>>();
        let vectors = vec![vec![0.8, 0.6], vec![1.0, 0.0], vec![0.0, 1.0]];
        let query = Query {
            id: "judged".into(),
            text: String::new(),
            relevant: vec!["positive".into()],
            task: GemmaTextTask::Code,
            eligible: Some(vec!["positive".into(), "negative".into()]),
        };
        let result = metrics(&documents, &vectors, &[query], &[vec![1.0, 0.0]], 2).unwrap();
        assert_eq!(result["hitAt1"], 1.0);
        assert_eq!(
            result["cases"][0]["topIds"],
            json!(["positive", "negative"])
        );

        let invalid = Query {
            id: "unjudged-positive".into(),
            text: String::new(),
            relevant: vec!["unjudged".into()],
            task: GemmaTextTask::Code,
            eligible: Some(vec!["positive".into(), "negative".into()]),
        };
        assert!(metrics(&documents, &vectors, &[invalid], &[vec![1.0, 0.0]], 2).is_err());
    }

    #[test]
    fn generated_media_fixture_has_labelled_color_and_shape_distractors() {
        let fixture = image_fixture().unwrap();
        assert_eq!(fixture.len(), 8);
        assert!(
            fixture
                .iter()
                .all(|subject| subject.png.starts_with(b"\x89PNG\r\n\x1a\n"))
        );
    }

    #[test]
    fn median_averages_middle_measurements_for_even_sample_counts() {
        assert_eq!(median(&[1.0, 2.0, 4.0, 8.0]), 3.0);
        assert_eq!(median(&[1.0, 2.0, 4.0]), 2.0);
    }
}
