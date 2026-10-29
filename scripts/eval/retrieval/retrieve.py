# /// script
# requires-python = ">=3.11"
# dependencies = ["numpy", "onnxruntime", "tokenizers"]
# ///
"""Retrieval check for chunk-size candidates with multilingual-e5-small (ONNX).

Reads the chunk dumps written by `corpus_chunk_stats` (`dump_chunks`), embeds every chunk with
`passage: ` and every query with `query: `, and reports hit@k and MRR per candidate maximum. A
chunk is relevant when it belongs to the query's document and contains the query's evidence
phrase. Chunks longer than the model limit are truncated, and their share is reported.
"""

import argparse
import json
import re
from pathlib import Path

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

MODEL_LIMIT = 512


def normalise(text: str) -> str:
    return re.sub(r"\s+", " ", text.lower())


def embed(
    session: ort.InferenceSession,
    tokenizer: Tokenizer,
    texts: list[str],
    batch: int = 16,
) -> np.ndarray:
    names = {i.name for i in session.get_inputs()}
    vectors = []
    for start in range(0, len(texts), batch):
        encoded = tokenizer.encode_batch(texts[start : start + batch])
        width = max(len(e.ids) for e in encoded)
        ids = np.zeros((len(encoded), width), dtype=np.int64)
        mask = np.zeros_like(ids)
        for row, e in enumerate(encoded):
            ids[row, : len(e.ids)] = e.ids
            mask[row, : len(e.ids)] = 1
        feed = {"input_ids": ids, "attention_mask": mask}
        if "token_type_ids" in names:
            feed["token_type_ids"] = np.zeros_like(ids)
        hidden = session.run(None, feed)[0]
        pooled = (hidden * mask[..., None]).sum(1) / mask.sum(1, keepdims=True)
        vectors.append(pooled / np.linalg.norm(pooled, axis=1, keepdims=True))
    return np.vstack(vectors)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--chunks", type=Path, required=True, help="dir with one sub-dir per maximum"
    )
    parser.add_argument(
        "--model",
        type=Path,
        required=True,
        help="dir with model.onnx and tokenizer.json",
    )
    parser.add_argument(
        "--queries", type=Path, default=Path(__file__).with_name("queries.json")
    )
    args = parser.parse_args()

    tokenizer = Tokenizer.from_file(str(args.model / "tokenizer.json"))
    tokenizer.enable_truncation(max_length=MODEL_LIMIT)
    tokenizer.enable_padding()
    session = ort.InferenceSession(
        str(args.model / "model.onnx"), providers=["CPUExecutionProvider"]
    )
    queries = json.loads(args.queries.read_text())
    query_vectors = embed(session, tokenizer, [f"query: {q['q']}" for q in queries])
    untruncated = Tokenizer.from_file(str(args.model / "tokenizer.json"))

    print(
        "| max (est. tokens) | chunks | truncated | answerable | hit@1 | hit@3 | hit@5 | MRR |"
    )
    print("|---|---|---|---|---|---|---|---|")
    for directory in sorted(
        (p for p in args.chunks.iterdir() if p.is_dir()), key=lambda p: int(p.name)
    ):
        chunks = [
            json.loads(line)
            for file in sorted(directory.glob("*.jsonl"))
            for line in file.read_text().splitlines()
        ]
        texts = [c["text"] for c in chunks]
        truncated = sum(
            len(e.ids) > MODEL_LIMIT
            for e in untruncated.encode_batch([f"passage: {t}" for t in texts])
        )
        vectors = embed(session, tokenizer, [f"passage: {t}" for t in texts])
        scores = query_vectors @ vectors.T
        hits = {1: 0, 3: 0, 5: 0}
        reciprocal = 0.0
        answerable = 0
        for row, query in enumerate(queries):
            relevant = {
                i
                for i, c in enumerate(chunks)
                if c["doc"].startswith(query["doc"])
                and normalise(query["evidence"]) in normalise(c["text"])
            }
            if not relevant:
                continue
            answerable += 1
            order = np.argsort(-scores[row])
            rank = next(r for r, i in enumerate(order, 1) if i in relevant)
            reciprocal += 1.0 / rank
            for k in hits:
                hits[k] += rank <= k
        n = max(answerable, 1)
        print(
            f"| {directory.name} | {len(chunks)} | {truncated} | {answerable}/{len(queries)} | "
            f"{hits[1] / n:.2f} | {hits[3] / n:.2f} | {hits[5] / n:.2f} | {reciprocal / n:.3f} |"
        )


if __name__ == "__main__":
    main()
