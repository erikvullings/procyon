"""Model-level E5 CPU ranking over the exact same labelled JSON as the native Gemma probe."""

import hashlib
import json
import sys
from pathlib import Path

GRAPH_SHA256 = "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665"
TOKENIZER_SHA256 = "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39"
REVISION = "614241f622f53c4eeff9890bdc4f31cfecc418b3"


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def rank_cases(corpus, vectors, query_vectors):
    import numpy as np

    documents = corpus["documents"]
    ids = [document["id"] for document in documents]
    if len(ids) != len(set(ids)) or len(vectors) != len(ids):
        raise ValueError("documents or vectors are duplicated or missing")
    if len(query_vectors) != len(corpus["cases"]):
        raise ValueError("query vectors are missing")
    results = []
    for case, query_vector in zip(corpus["cases"], query_vectors, strict=True):
        relevant = set(case["relevantIds"])
        eligible = set(case["eligibleIds"])
        if not relevant or not relevant <= eligible or not eligible <= set(ids):
            raise ValueError(f"{case['id']} has invalid or unjudged relevant labels")
        candidates = [index for index, identifier in enumerate(ids) if identifier in eligible]
        ranked = sorted(candidates, key=lambda index: (-float(np.dot(vectors[index], query_vector)), index))
        ranked_ids = [ids[index] for index in ranked]
        first = next((index + 1 for index, identifier in enumerate(ranked_ids) if identifier in relevant), None)
        results.append({
            "id": case["id"],
            "firstRelevantRank": first,
            "topIds": ranked_ids[:3],
        })
    return {
        "positiveCases": len(results),
        "hitAt1": sum(case["firstRelevantRank"] == 1 for case in results) / len(results),
        "hitAt3": sum(case["firstRelevantRank"] is not None and case["firstRelevantRank"] <= 3 for case in results) / len(results),
        "mrr": sum(1 / case["firstRelevantRank"] if case["firstRelevantRank"] else 0 for case in results) / len(results),
        "cases": results,
    }


def evaluate(model_dir, fixture_path):
    import numpy as np
    import onnxruntime as ort
    from tokenizers import Tokenizer

    graph = model_dir / "model.onnx"
    tokenizer_file = model_dir / "tokenizer.json"
    if digest(graph) != GRAPH_SHA256 or digest(tokenizer_file) != TOKENIZER_SHA256:
        raise ValueError("E5 original member digest does not match the signed candidate")
    raw = fixture_path.read_bytes()
    corpus = json.loads(raw)
    documents, cases = corpus["documents"], corpus["cases"]
    if not documents or not cases:
        raise ValueError("empty benchmark")
    tokenizer = Tokenizer.from_file(str(tokenizer_file))
    tokenizer.enable_truncation(max_length=512)
    tokenizer.enable_padding(pad_id=tokenizer.token_to_id("<pad>"), pad_token="<pad>")
    session_options = ort.SessionOptions()
    session_options.intra_op_num_threads = 4
    session_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    session = ort.InferenceSession(str(graph), session_options, providers=["CPUExecutionProvider"])
    names = {item.name for item in session.get_inputs()}
    if not {"input_ids", "attention_mask"} <= names:
        raise ValueError("E5 graph has unexpected inputs")

    def encode(texts):
        vectors = []
        for text in texts:
            encoded = tokenizer.encode(text, add_special_tokens=True)
            ids = np.asarray([encoded.ids], dtype=np.int64)
            mask = np.asarray([encoded.attention_mask], dtype=np.int64)
            inputs = {"input_ids": ids, "attention_mask": mask}
            if "token_type_ids" in names:
                inputs["token_type_ids"] = np.zeros_like(ids)
            states = session.run(["last_hidden_state"], inputs)[0][0]
            vector = np.sum(states * mask[0, :, None], axis=0) / np.sum(mask)
            norm = np.linalg.norm(vector)
            if norm == 0 or not np.all(np.isfinite(vector)):
                raise ValueError("E5 inference produced an invalid vector")
            vectors.append(vector / norm)
        return vectors

    result = rank_cases(
        corpus,
        encode(["passage: " + document["text"] for document in documents]),
        encode(["query: " + case["query"] for case in cases]),
    )
    return {
        "model": "intfloat/multilingual-e5-small",
        "revision": REVISION,
        "runtime": f"Python onnxruntime {ort.__version__} CPU mean-pooling",
        "dimensions": 384,
        "corpusSha256": hashlib.sha256(raw).hexdigest(),
        "documentCount": len(documents),
        "queryCount": len(cases),
        "result": result,
    }


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit("usage: evaluate_e5_quality.py SIGNED_E5_MEMBERS LABELLED_FIXTURE_JSON")
    print(json.dumps(evaluate(Path(sys.argv[1]), Path(sys.argv[2])), indent=2))
