"""Generate offline Gemma4 audio-tower goldens from the pinned checkpoint."""

import json
import sys
from pathlib import Path

import torch
from safetensors import safe_open
from transformers.models.embedding_gemma2.configuration_embedding_gemma2 import EmbeddingGemma2Config
from transformers.models.gemma4.modeling_gemma4 import Gemma4AudioModel

REVISION = "914f7f89142e33e77833254d9c9b90c3cef7303b"


def main(directory: Path, output: Path) -> None:
    assert directory.name == REVISION
    config = EmbeddingGemma2Config.from_pretrained(directory, local_files_only=True)
    model = Gemma4AudioModel(config.audio_config).float()
    with safe_open(directory / "model.safetensors", framework="pt", device="cpu") as checkpoint:
        weights = {
            name.removeprefix("audio_tower."): checkpoint.get_tensor(name).float()
            for name in checkpoint.keys()
            if name.startswith("audio_tower.")
        }
    model.load_state_dict(weights, strict=True)
    model.eval()
    cases = []
    for frame_count, valid_frames in ((16, 16), (29, 21)):
        features = [
            ((frame * 19 + band * 7) % 31 - 15) / 16.0
            if frame < valid_frames else 0.0
            for frame in range(frame_count)
            for band in range(128)
        ]
        with torch.inference_mode():
            result = model(
                torch.tensor(features).reshape(1, frame_count, 128),
                attention_mask=torch.tensor([[frame < valid_frames for frame in range(frame_count)]]),
            )
        cases.append(
            {
                "frame_count": frame_count,
                "valid_frames": valid_frames,
                "features": features,
                "output_mask": result.attention_mask[0].tolist(),
                "output": result.last_hidden_state[0].tolist(),
            }
        )
    output.write_text(json.dumps({"revision": REVISION, "cases": cases}) + "\n")


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve(), Path(sys.argv[2]))
