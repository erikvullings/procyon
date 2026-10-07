"""CPU-only, model-level retrieval comparison on the repository-owned corpus.

Run each model in a separate process so construction and peak RSS are attributable.
This is not the production Zvec/hybrid retrieval or an eligibility/threshold test.
"""

import argparse
import hashlib
import io
import json
import math
import platform
import resource
import statistics
import tempfile
import time
import wave
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageDraw
from sentence_transformers import SentenceTransformer

CORPUS = Path(__file__).resolve().parent.parent / (
    "crates/fm-application/tests/fixtures/knowledge-retrieval-corpus-v1.json"
)
MODELS = {
    "e5": ("intfloat/multilingual-e5-small", "614241f622f53c4eeff9890bdc4f31cfecc418b3"),
    "gemma": ("google/embeddinggemma-2", "914f7f89142e33e77833254d9c9b90c3cef7303b"),
}


def peak_rss_bytes():
    maximum = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return maximum * 1024 if platform.system() == "Linux" else maximum


def encode(model, model_name, texts, is_query):
    if model_name == "e5":
        texts = [("query: " if is_query else "passage: ") + text.casefold() for text in texts]
        return model.encode(texts, batch_size=8 if not is_query else 1, normalize_embeddings=True)
    return model.encode(
        texts,
        batch_size=8 if not is_query else 1,
        prompt_name="SearchQuery" if is_query else "Document",
        normalize_embeddings=True,
    )


def metrics(cases, rows, document_vectors, query_vectors, dimensions):
    docs = document_vectors[:, :dimensions]
    docs = docs / np.linalg.norm(docs, axis=1, keepdims=True)
    queries = query_vectors[:, :dimensions]
    queries = queries / np.linalg.norm(queries, axis=1, keepdims=True)
    scored = []
    for case, query in zip(cases, queries, strict=True):
        if case["expectedNoEvidence"]:
            continue  # no calibrated acceptance threshold: never pretend to measure rejection
        order = np.argsort(-(docs @ query), kind="stable").tolist()
        chunks = [rows[i]["chunkId"] for i in order]
        sources = list(dict.fromkeys(
            source for i in order for source in rows[i]["sourceIds"]
        ))
        relevant_chunks = set(case["relevantChunkIds"])
        relevant_sources = set(case["relevantSourceIds"])
        ranked_sources = sources[:10]
        ranked_chunks = chunks[:10]
        first = next(
            (rank for rank, source in enumerate(sources, start=1) if source in relevant_sources),
            None,
        )
        gains = [
            int(source in relevant_sources) / math.log2(rank + 2)
            for rank, source in enumerate(ranked_sources)
        ]
        ideal = sum(
            1 / math.log2(rank + 2)
            for rank in range(min(len(relevant_sources), 10))
        )
        scored.append({
            "id": case["id"],
            "fileRecallAt10": len(relevant_sources.intersection(ranked_sources)) / len(relevant_sources),
            "chunkRecallAt10": len(relevant_chunks.intersection(ranked_chunks)) / len(relevant_chunks),
            "reciprocalRank": 1 / first if first else 0,
            "ndcgAt10": sum(gains) / ideal,
            "topChunkIds": ranked_chunks,
        })
    return {
        "evaluatedPositiveCases": len(scored),
        "excludedNegativeCases": len(cases) - len(scored),
        "fileRecallAt10": statistics.mean(row["fileRecallAt10"] for row in scored),
        "chunkRecallAt10": statistics.mean(row["chunkRecallAt10"] for row in scored),
        "mrr": statistics.mean(row["reciprocalRank"] for row in scored),
        "ndcgAt10": statistics.mean(row["ndcgAt10"] for row in scored),
        "cases": scored,
    }


def image_smoke(model):
    original = Image.new("RGB", (256, 256), "white")
    draw = ImageDraw.Draw(original)
    draw.rectangle((40, 55, 220, 195), fill="#ed533a")
    draw.ellipse((95, 85, 165, 155), fill="#ffda57")
    resized = original.resize((128, 128))
    output = io.BytesIO()
    resized.save(output, format="JPEG", quality=70)
    duplicate = Image.open(io.BytesIO(output.getvalue())).convert("RGB")
    unrelated = Image.new("RGB", (256, 256), "#1557b0")
    ImageDraw.Draw(unrelated).polygon([(128, 20), (236, 220), (20, 220)], fill="#49d5a3")
    start = time.perf_counter()
    vectors = np.stack([
        model.encode({"image": image}, normalize_embeddings=True)
        for image in (original, duplicate, unrelated)
    ])
    return {
        "source": "deterministically generated geometric images; JPEG resized duplicate",
        "imageCount": 3,
        "seconds": time.perf_counter() - start,
        "duplicateCosine": float(vectors[0] @ vectors[1]),
        "unrelatedCosine": float(vectors[0] @ vectors[2]),
        "duplicateRanksFirst": bool(vectors[0] @ vectors[1] > vectors[0] @ vectors[2]),
    }


def audio_smoke(model):
    with tempfile.TemporaryDirectory(prefix="embeddinggemma-av-") as directory:
        root = Path(directory)
        audio = root / "tone.wav"
        with wave.open(str(audio), "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(16_000)
            samples = np.arange(16_000)
            tone = (np.sin(2 * np.pi * 440 * samples / 16_000) * 8000).astype("<i2")
            output.writeframes(tone.tobytes())
        start = time.perf_counter()
        vector = model.encode({"audio": str(audio)}, normalize_embeddings=True)
        return {
            "source": "generated one-second 440 Hz 16 kHz mono WAV",
            "seconds": time.perf_counter() - start,
            "dimensions": len(vector),
            "finiteUnitNorm": bool(np.isfinite(vector).all() and
                                   abs(np.linalg.norm(vector) - 1) < 1e-4),
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("model", choices=MODELS)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--image-smoke", action="store_true")
    parser.add_argument("--audio-smoke", action="store_true")
    args = parser.parse_args()
    if (args.image_smoke or args.audio_smoke) and args.model != "gemma":
        parser.error("media smoke requires Gemma; E5 has no media encoders")
    torch.set_num_threads(4)
    model_id, revision = MODELS[args.model]
    corpus_bytes = CORPUS.read_bytes()
    corpus = json.loads(corpus_bytes)
    rows = []
    for document in corpus["documents"]:
        source_ids = [
            occurrence["sourceId"] for occurrence in document["occurrences"]
            if occurrence["rootId"] in corpus["authorizedRootIds"] and occurrence["available"]
        ]
        for chunk in document["chunks"]:
            if source_ids:
                rows.append({
                    "chunkId": chunk["chunkId"],
                    "sourceIds": source_ids,
                    "text": " > ".join(chunk["sectionPath"]) + "\n" + chunk["text"],
                })
    cases = corpus["cases"]
    start = time.perf_counter()
    model = SentenceTransformer(
        model_id,
        revision=revision,
        device="cpu",
        model_kwargs={"torch_dtype": torch.float32},
        **({"config_kwargs": ({"vision_config": None} if args.audio_smoke else
                               {"audio_config": None} if args.image_smoke else
                               {"vision_config": None, "audio_config": None})}
           if args.model == "gemma" else {}),
    )
    model_construction_seconds = time.perf_counter() - start
    if args.image_smoke or args.audio_smoke:
        report = {
            "model": model_id,
            "revision": revision,
            "modelConstructionSeconds": model_construction_seconds,
            "peakRssBytes": None,
            "result": audio_smoke(model) if args.audio_smoke else image_smoke(model),
        }
        report["peakRssBytes"] = peak_rss_bytes()
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
        return
    start = time.perf_counter()
    documents = encode(model, args.model, [row["text"] for row in rows], False)
    document_seconds = time.perf_counter() - start
    query_latencies = []
    queries = []
    for case in cases:
        start = time.perf_counter()
        queries.append(encode(model, args.model, [case["question"]], True)[0])
        query_latencies.append(time.perf_counter() - start)
    queries = np.stack(queries)
    dimensions = [384] if args.model == "e5" else [128, 256, 512, 768]
    report = {
        "model": model_id,
        "revision": revision,
        "corpusSha256": hashlib.sha256(corpus_bytes).hexdigest(),
        "corpusId": corpus["corpusId"],
        "hardware": platform.platform() + " " + platform.machine(),
        "torch": torch.__version__,
        "sentenceTransformers": __import__("sentence_transformers").__version__,
        "transformers": __import__("transformers").__version__,
        "cpuThreads": torch.get_num_threads(),
        "precision": "float32",
        "measurements": {
            "modelConstructionSeconds": model_construction_seconds,
            "documentCount": len(rows),
            "documentBatchSeconds": document_seconds,
            "documentsPerSecond": len(rows) / document_seconds,
            "queryLatencyMedianMs": statistics.median(query_latencies) * 1000,
            "queryLatencyP95Ms": sorted(query_latencies)[math.ceil(0.95 * len(queries)) - 1] * 1000,
            "peakRssBytes": peak_rss_bytes(),
        },
        "dimensions": {
            str(dim): metrics(cases, rows, documents, queries, dim) for dim in dimensions
        },
    }
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "dimensions"}, indent=2))
    print(json.dumps({
        dim: {k: v for k, v in score.items() if k != "cases"}
        for dim, score in report["dimensions"].items()
    }, indent=2))


if __name__ == "__main__":
    main()
