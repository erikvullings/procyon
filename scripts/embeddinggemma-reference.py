"""Generate a CPU reference vector from an already downloaded EmbeddingGemma 2 checkpoint.

No model downloads or remote code execution occur; this is development-only
comparison input for the gated Rust probe, not a production inference path.
"""

import argparse
import json
from pathlib import Path

import torch
from sentence_transformers import SentenceTransformer

REVISION = "914f7f89142e33e77833254d9c9b90c3cef7303b"
FIXTURE_CASES = (
    {"task": "search", "dimensions": 768, "text": "What causes the northern lights?"},
    {"task": "question", "dimensions": 512, "text": "Hoe ontstaat het noorderlicht?"},
    {"task": "code", "dimensions": 256, "text": "Find the function that copies files"},
    {
        "task": "document",
        "dimensions": 128,
        "text": "The northern lights are caused by charged particles from the sun.",
        "title": "Aurora notes",
    },
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("dimensions", type=int, nargs="?", choices=(128, 256, 512, 768))
    parser.add_argument("task", nargs="?", choices=("search", "question", "code", "document"))
    parser.add_argument("text", nargs="?")
    parser.add_argument("--title")
    parser.add_argument("--fixtures", action="store_true")
    args = parser.parse_args()
    if args.fixtures and any(item is not None for item in (args.dimensions, args.task, args.text, args.title)):
        parser.error("--fixtures takes only a model directory")
    if not args.fixtures and any(item is None for item in (args.dimensions, args.task, args.text)):
        parser.error("dimensions, task, and text are required without --fixtures")
    if args.title is not None and args.task != "document":
        parser.error("--title requires document task")
    prompts = {
        "search": "SearchQuery",
        "question": "QuestionAnswering",
        "code": "CodeRetrieval",
        "document": "Document",
    }
    torch.set_num_threads(4)
    model = SentenceTransformer(
        str(args.directory),
        device="cpu",
        local_files_only=True,
        model_kwargs={"torch_dtype": torch.float32},
        config_kwargs={"vision_config": None, "audio_config": None},
    )
    cases = FIXTURE_CASES if args.fixtures else ({
        "task": args.task,
        "dimensions": args.dimensions,
        "text": args.text,
        **({"title": args.title} if args.title is not None else {}),
    },)
    output = []
    for case in cases:
        title = case.get("title")
        text = f"title: {title} | text: {case['text']}" if title is not None else case["text"]
        vector = model.encode(
            text,
            prompt_name=None if title is not None else prompts[case["task"]],
            truncate_dim=case["dimensions"],
            normalize_embeddings=True,
        )
        output.append({**case, "vector": vector.tolist()})
    print(json.dumps({"revision": REVISION, "cases": output} if args.fixtures else output[0]["vector"]))


if __name__ == "__main__":
    main()
