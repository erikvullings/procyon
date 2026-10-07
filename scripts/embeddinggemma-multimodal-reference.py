"""Generate vision/audio projection goldens from a local pinned checkpoint."""

import argparse
import json
from pathlib import Path

import torch
from safetensors import safe_open

REVISION = "914f7f89142e33e77833254d9c9b90c3cef7303b"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    cases = []
    with safe_open(args.directory / "model.safetensors", framework="pt", device="cpu") as tensors:
        for modality, width, eps in (("vision", 768, 1e-6), ("audio", 1536, 1e-6)):
            name = f"embed_{modality}.embedding_projection.weight"
            if tensors.get_slice(name).get_shape() != [512, width]:
                raise ValueError(f"unexpected projection dimensions: {name}")
            weights = tensors.get_tensor(name)
            if weights.dtype != torch.bfloat16:
                raise ValueError(f"expected BF16 checkpoint weights, got {weights.dtype}")
            token = torch.linspace(-1.0, 1.0, width)
            normalized = token * torch.rsqrt(token.square().mean() + eps)
            output = torch.nn.functional.linear(normalized, weights.float())
            cases.append({
                "modality": modality,
                "input": token.tolist(),
                "output": output.tolist(),
            })

    args.output.write_text(json.dumps({"revision": REVISION, "cases": cases}, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
