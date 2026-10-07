"""Attempt a local ONNX export of EmbeddingGemma 2's text tower.

This is a feasibility experiment, not an installable or validated component.
The test deliberately uses only the text encoder; media export must be proved
independently before a production decision.
"""

import argparse
import json
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from transformers import AutoConfig, AutoModel, AutoTokenizer

REFERENCE = Path(__file__).resolve().parents[1] / "crates/fm-semantic-worker/tests/embeddinggemma-reference-v1.json"
PROMPTS = {
    "search": "task: search result | query: ",
    "question": "task: question answering | query: ",
    "code": "task: code retrieval | query: ",
}


class TextEmbedding(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, input_ids, attention_mask):
        states = self.model(input_ids=input_ids, attention_mask=attention_mask).last_hidden_state
        weights = attention_mask.unsqueeze(-1).to(states.dtype)
        pooled = (states * weights).sum(dim=1) / weights.sum(dim=1)
        return F.normalize(pooled, p=2, dim=-1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--dynamic", action="store_true", help="export variable-length text inputs")
    args = parser.parse_args()

    cases = json.loads(REFERENCE.read_text())["cases"]
    torch.set_num_threads(4)
    config = AutoConfig.from_pretrained(args.directory, local_files_only=True)
    config.vision_config = None
    config.audio_config = None
    model = AutoModel.from_pretrained(
        args.directory, config=config, local_files_only=True, dtype=torch.float32
    ).eval()
    tokenizer = AutoTokenizer.from_pretrained(args.directory, local_files_only=True)
    first = cases[0]
    inputs = tokenizer(PROMPTS[first["task"]] + first["text"], return_tensors="pt")
    wrapper = TextEmbedding(model).eval()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with torch.no_grad():
        torch.onnx.export(
            wrapper,
            (inputs["input_ids"], inputs["attention_mask"]),
            str(args.output),
            input_names=("input_ids", "attention_mask"),
            output_names=("sentence_vector",),
            dynamo=True,
            external_data=True,
            dynamic_shapes=(
                {
                    "input_ids": {1: torch.export.Dim("sequence", min=2, max=512)},
                    "attention_mask": {1: torch.export.Dim("sequence", min=2, max=512)},
                }
                if args.dynamic
                else None
            ),
        )

    import onnxruntime as ort

    session = ort.InferenceSession(str(args.output), providers=["CPUExecutionProvider"])
    for case in cases if args.dynamic else cases[:1]:
        if case["task"] == "document":
            text = f"title: {case.get('title', 'none')} | text: {case['text']}"
        else:
            text = PROMPTS[case["task"]] + case["text"]
        encoded = tokenizer(text, return_tensors="pt")
        actual = session.run(
            ["sentence_vector"],
            {name: encoded[name].numpy() for name in ("input_ids", "attention_mask")},
        )[0][0, : case["dimensions"]]
        actual /= np.linalg.norm(actual)
        reference = np.asarray(case["vector"])
        difference = np.max(np.abs(actual - reference))
        print(f"{case['task']} ({encoded['input_ids'].shape[1]} tokens, {case['dimensions']}d): max absolute difference {difference}")
        if difference > 1e-4:
            raise AssertionError(f"{case['task']} ONNX output diverges from upstream Python reference")


if __name__ == "__main__":
    main()
