"""Generate Gemma vision-preprocessing reference from deterministic RGB images."""

import argparse
import io
import json
from pathlib import Path

import numpy as np
from PIL import Image
from transformers.models.gemma4.image_processing_gemma4 import Gemma4ImageProcessor


def image_at(width, height):
    pixels = np.fromfunction(
        lambda y, x, c: np.where(
            c == 0, (x * 2 + y) % 256, np.where(c == 1, (x + y * 2) % 256, (x + y) % 256)
        ),
        (height, width, 3),
    ).astype("uint8")
    image = Image.fromarray(pixels, "RGB")
    buffer = io.BytesIO()
    image.save(buffer, format="PNG")
    return Image.open(io.BytesIO(buffer.getvalue()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    settings = json.loads((args.directory / "processor_config.json").read_text())
    cases = []
    for kind, width, height in (
        ("image", 128, 96),
        ("image", 512, 384),
        ("image", 2048, 1536),
        ("video", 128, 96),
        ("video", 512, 384),
        ("video", 2048, 1536),
    ):
        options = settings["image_processor" if kind == "image" else "video_processor"]
        processor = Gemma4ImageProcessor(
            patch_size=options["patch_size"],
            max_soft_tokens=options["max_soft_tokens"],
            pooling_kernel_size=options["pooling_kernel_size"],
            resample=options["resample"],
        )
        result = processor(images=image_at(width, height), return_tensors="pt")
        pixel_values = result["pixel_values"][0]
        positions = result["image_position_ids"][0]
        flattened = pixel_values.flatten()
        indices = sorted(
            set([0, 1, 2, 15, 16, 767, len(flattened) // 2] +
                [index * (len(flattened) - 1) // 2047 for index in range(2048)])
        )
        cases.append({
            "kind": kind,
            "width": width,
            "height": height,
            "soft_tokens": int(result["num_soft_tokens_per_image"][0]),
            "valid_patches": int((positions[:, 0] >= 0).sum()),
            "positions": [positions[index].tolist() for index in (0, 1, 50, 500, len(positions) - 1)],
            "values": [[index, flattened[index].item()] for index in indices],
        })
    args.output.write_text(json.dumps(cases, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
