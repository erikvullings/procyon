"""Generate offline vision-tower goldens using pinned Transformers and checkpoint.

Run with target/embeddinggemma-probe/.venv/bin/python and a local model path.
This script does not download model data or serve inference.
"""

import json
import sys
from pathlib import Path

import torch
from safetensors import safe_open
from transformers.models.gemma4.configuration_gemma4 import Gemma4VisionConfig
from transformers.models.gemma4.modeling_gemma4 import Gemma4VisionModel


def image_patches(frame: int) -> tuple[torch.Tensor, torch.Tensor]:
    # A reproducible 48x48 RGB image (or video frame), fed through the
    # upstream HWC patchification order without any resize.
    width = 96 if frame == 3 else 48
    rgb = torch.tensor(
        [
            [
                [(x * 7 + y * 3 + channel * 53 + frame * 29) % 256 for channel in range(3)]
                for x in range(width)
            ]
            for y in range(48)
        ],
        dtype=torch.float32,
    ) / 255.0
    patch_width = width // 16
    patches = rgb.reshape(3, 16, patch_width, 16, 3).permute(0, 2, 1, 3, 4)
    patches = patches.reshape(1, 3 * patch_width, 768)
    positions = torch.tensor([[x, y] for y in range(3) for x in range(patch_width)])[None]
    return patches, positions


def main() -> None:
    directory = Path(sys.argv[1]).resolve()
    config = json.loads((directory / "config.json").read_text())
    model = Gemma4VisionModel(Gemma4VisionConfig(**config["vision_config"]))
    with safe_open(directory / "model.safetensors", framework="pt", device="cpu") as checkpoint:
        weights = {
            key.removeprefix("vision_tower."): checkpoint.get_tensor(key).float()
            for key in checkpoint.keys()
            if key.startswith("vision_tower.")
        }
    model.load_state_dict(weights, strict=True)
    model.eval()
    cases = []
    with torch.no_grad():
        for frame in (0, 1, 2, 3):
            patches, positions = image_patches(frame)
            if frame == 2:
                patches = torch.cat([patches, torch.zeros_like(patches)], dim=1)
                positions = torch.cat([positions, torch.full_like(positions, -1)], dim=1)
            output = model(pixel_values=patches, pixel_position_ids=positions).last_hidden_state
            cases.append({"frame": frame, "output": output.tolist()})
    result = {
        "revision": directory.name,
        "transformers": "5.19.0",
        "cases": cases,
    }
    path = Path("crates/fm-semantic-worker/tests/embeddinggemma-vision-reference-v1.json")
    path.write_text(json.dumps(result, separators=(",", ":")) + "\n")
    print(f"Wrote {path}, {len(cases)} real-checkpoint frames")


if __name__ == "__main__":
    main()
