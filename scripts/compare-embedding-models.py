"""Model-level retrieval comparison on repository-owned/generated corpora.

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
CODE_CORPUS = Path(__file__).resolve().parent / "fixtures/embedding-code-search-v1.json"
MODELS = {
    "e5": ("intfloat/multilingual-e5-small", "614241f622f53c4eeff9890bdc4f31cfecc418b3"),
    "gemma": ("google/embeddinggemma-2", "914f7f89142e33e77833254d9c9b90c3cef7303b"),
}


def peak_rss_bytes():
    maximum = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    return maximum * 1024 if platform.system() == "Linux" else maximum


def encode(model, model_name, texts, is_query, task="search"):
    if model_name == "e5":
        texts = [("query: " if is_query else "passage: ") + text.casefold() for text in texts]
        return model.encode(texts, batch_size=8 if not is_query else 1, normalize_embeddings=True)
    return model.encode(
        texts,
        batch_size=8 if not is_query else 1,
        prompt_name=("CodeRetrieval" if task == "code" else "SearchQuery") if is_query else "Document",
        normalize_embeddings=True,
    )


def synchronize(device):
    if device == "mps":
        torch.mps.synchronize()


def code_metrics(cases, rows, document_vectors, query_vectors, dimensions):
    docs = document_vectors[:, :dimensions]
    docs = docs / np.linalg.norm(docs, axis=1, keepdims=True)
    queries = query_vectors[:, :dimensions]
    queries = queries / np.linalg.norm(queries, axis=1, keepdims=True)
    scores = []
    for case, query in zip(cases, queries, strict=True):
        ranked = [rows[i]["id"] for i in np.argsort(-(docs @ query), kind="stable")]
        relevant = set(case["relevantIds"])
        first = next((i for i, key in enumerate(ranked, 1) if key in relevant), None)
        scores.append({
            "id": case["id"],
            "recallAt1": len(relevant.intersection(ranked[:1])) / len(relevant),
            "recallAt3": len(relevant.intersection(ranked[:3])) / len(relevant),
            "reciprocalRank": 1 / first if first else 0,
            "topIds": ranked[:3],
        })
    return {
        "caseCount": len(scores),
        "recallAt1": statistics.mean(score["recallAt1"] for score in scores),
        "recallAt3": statistics.mean(score["recallAt3"] for score in scores),
        "mrr": statistics.mean(score["reciprocalRank"] for score in scores),
        "cases": scores,
    }


def image_fixture():
    styles = [
        ("red-circle", "#e52b32", "circle"),
        ("blue-square", "#1750db", "square"),
        ("green-triangle", "#1aa45b", "triangle"),
        ("yellow-circle", "#e0bb19", "circle"),
    ]
    images = []
    for label, color, shape in styles:
        image = Image.new("RGB", (224, 224), "white")
        draw = ImageDraw.Draw(image)
        if shape == "circle":
            draw.ellipse((42, 42, 182, 182), fill=color)
        elif shape == "square":
            draw.rectangle((42, 42, 182, 182), fill=color)
        else:
            draw.polygon([(112, 30), (195, 185), (29, 185)], fill=color)
        compressed = io.BytesIO()
        image.resize((112, 112)).save(compressed, "JPEG", quality=65)
        images.extend([
            (label + "-original", image),
            (label + "-jpeg", Image.open(io.BytesIO(compressed.getvalue())).convert("RGB")),
        ])
    for label, color, shape in (("red-square", "#e52b32", "square"),
                                 ("blue-circle", "#1750db", "circle"),
                                 ("green-circle", "#1aa45b", "circle"),
                                 ("yellow-square", "#e0bb19", "square")):
        image = Image.new("RGB", (224, 224), "white")
        draw = ImageDraw.Draw(image)
        if shape == "circle":
            draw.ellipse((42, 42, 182, 182), fill=color)
        else:
            draw.rectangle((42, 42, 182, 182), fill=color)
        images.append((label, image))
    return images


def image_metrics(images, document_vectors, queries):
    scores = []
    for label, query_vector, exclude_self in queries:
        ranked = [
            images[i][0] for i in np.argsort(-(document_vectors @ query_vector), kind="stable")
            if not exclude_self or images[i][0] != label + "-original"
        ]
        relevant = {label + "-jpeg"} if exclude_self else {label + "-original", label + "-jpeg"}
        first = next((i for i, key in enumerate(ranked, 1) if key in relevant), None)
        scores.append({
            "id": label,
            "recallAt2": len(relevant.intersection(ranked[:2])) / len(relevant),
            "reciprocalRank": 1 / first if first else 0,
            "topIds": ranked[:3],
        })
    return {
        "caseCount": len(scores),
        "recallAt2": statistics.mean(score["recallAt2"] for score in scores),
        "mrr": statistics.mean(score["reciprocalRank"] for score in scores),
        "cases": scores,
    }


def image_retrieval(model, device):
    images = image_fixture()
    model.encode({"image": images[0][1]}, normalize_embeddings=True)
    model.encode("red circle on a white background", prompt_name="SearchQuery",
                 normalize_embeddings=True)
    synchronize(device)
    start = time.perf_counter()
    vectors = np.stack([model.encode({"image": image}, normalize_embeddings=True)
                        for _, image in images])
    synchronize(device)
    document_seconds = time.perf_counter() - start
    query_sets = {}
    query_latencies = []
    labels = ["red-circle", "blue-square", "green-triangle", "yellow-circle"]
    for kind in ("textToImage", "imageToImage"):
        queries = []
        for label in labels:
            start = time.perf_counter()
            if kind == "textToImage":
                vector = model.encode(label.replace("-", " ") + " on a white background",
                                      prompt_name="SearchQuery", normalize_embeddings=True)
            else:
                vector = vectors[next(i for i, (name, _) in enumerate(images)
                                      if name == label + "-original")]
            synchronize(device)
            if kind == "textToImage":
                query_latencies.append((time.perf_counter() - start) * 1000)
            queries.append((label, vector, kind == "imageToImage"))
        query_sets[kind] = queries
    return {
        "fixture": "four generated colored shapes with resized JPEG positives and same-color/shape negatives",
        "imageCount": len(images),
        "imageEncodeSeconds": document_seconds,
        "imagesPerSecond": len(images) / document_seconds,
        "medianTextQueryMs": statistics.median(query_latencies),
        "dimensions": {
            str(dim): {
                kind: image_metrics(
                    images, vectors[:, :dim] / np.linalg.norm(vectors[:, :dim], axis=1, keepdims=True),
                    [(label, vector[:dim] / np.linalg.norm(vector[:dim]), exclude)
                     for label, vector, exclude in queries],
                )
                for kind, queries in query_sets.items()
            }
            for dim in (128, 256, 512, 768)
        },
    }


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
    media = parser.add_mutually_exclusive_group()
    media.add_argument("--image-smoke", action="store_true")
    media.add_argument("--image-eval", action="store_true")
    media.add_argument("--audio-smoke", action="store_true")
    parser.add_argument("--corpus", choices=("knowledge", "code"), default="knowledge")
    parser.add_argument("--device", choices=("cpu", "mps"), default="cpu")
    args = parser.parse_args()
    if (args.image_smoke or args.image_eval or args.audio_smoke) and args.model != "gemma":
        parser.error("media smoke requires Gemma; E5 has no media encoders")
    if args.device == "mps" and not torch.backends.mps.is_available():
        parser.error("MPS unavailable; no GPU measurement produced")
    torch.set_num_threads(4)
    model_id, revision = MODELS[args.model]
    corpus_bytes = (CODE_CORPUS if args.corpus == "code" else CORPUS).read_bytes()
    corpus = json.loads(corpus_bytes)
    rows = []
    if args.corpus == "code":
        rows = corpus["documents"]
    else:
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
        device=args.device,
        model_kwargs={"torch_dtype": torch.float32},
        **({"config_kwargs": ({"vision_config": None} if args.audio_smoke else
                               {"audio_config": None} if args.image_smoke or args.image_eval else
                               {"vision_config": None, "audio_config": None})}
           if args.model == "gemma" else {}),
    )
    synchronize(args.device)
    model_construction_seconds = time.perf_counter() - start
    if args.image_smoke or args.image_eval or args.audio_smoke:
        report = {
            "model": model_id,
            "revision": revision,
            "device": args.device,
            "precision": "float32",
            "modelConstructionSeconds": model_construction_seconds,
            "peakRssBytes": None,
            "result": (audio_smoke(model) if args.audio_smoke else
                       image_retrieval(model, args.device) if args.image_eval else image_smoke(model)),
        }
        report["peakRssBytes"] = peak_rss_bytes()
        if args.device == "mps":
            report["mpsAllocatedBytesAfterRun"] = torch.mps.current_allocated_memory()
            report["mpsDriverBytesAfterRun"] = torch.mps.driver_allocated_memory()
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, indent=2))
        return
    if args.corpus == "code":
        encode(model, args.model, [row["text"] for row in rows], False)
        encode(model, args.model, [cases[0]["query"]], True, "code")
        synchronize(args.device)
    start = time.perf_counter()
    documents = encode(model, args.model, [row["text"] for row in rows], False)
    synchronize(args.device)
    document_seconds = time.perf_counter() - start
    query_latencies = []
    queries = []
    for case in cases:
        start = time.perf_counter()
        queries.append(encode(model, args.model,
                              [case["query"] if args.corpus == "code" else case["question"]],
                              True, args.corpus)[0])
        synchronize(args.device)
        query_latencies.append(time.perf_counter() - start)
    queries = np.stack(queries)
    dimensions = [384] if args.model == "e5" else [128, 256, 512, 768]
    report = {
        "model": model_id,
        "revision": revision,
        "corpusSha256": hashlib.sha256(corpus_bytes).hexdigest(),
        "corpusId": corpus["corpusId"],
        "device": args.device,
        "queryPrompt": ("query: " if args.model == "e5" else
                        "CodeRetrieval" if args.corpus == "code" else "SearchQuery"),
        "documentPrompt": "passage: " if args.model == "e5" else "Document",
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
            str(dim): (code_metrics(cases, rows, documents, queries, dim)
                       if args.corpus == "code" else metrics(cases, rows, documents, queries, dim))
            for dim in dimensions
        },
    }
    if args.device == "mps":
        report["measurements"]["mpsAllocatedBytesAfterRun"] = torch.mps.current_allocated_memory()
        report["measurements"]["mpsDriverBytesAfterRun"] = torch.mps.driver_allocated_memory()
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k != "dimensions"}, indent=2))
    print(json.dumps({
        dim: {k: v for k, v in score.items() if k != "cases"}
        for dim, score in report["dimensions"].items()
    }, indent=2))


if __name__ == "__main__":
    main()
