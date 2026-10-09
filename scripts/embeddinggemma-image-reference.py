"""Generate an upstream image vector for the native full-pipeline parity test."""

import argparse
import json
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from PIL import Image
from transformers import AutoModel, AutoProcessor


def image_at(width, height):
    pixels = np.fromfunction(
        lambda y, x, c: np.where(
            c == 0, (x * 2 + y) % 256, np.where(c == 1, (x + y * 2) % 256, (x + y) % 256)
        ),
        (height, width, 3),
    ).astype("uint8")
    return Image.fromarray(pixels, "RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--video", action="store_true", help="process the frame as a video")
    parser.add_argument("--two-frames", action="store_true", help="repeat the video frame")
    args = parser.parse_args()
    torch.set_num_threads(4)
    processor = AutoProcessor.from_pretrained(args.directory, local_files_only=True)
    model = AutoModel.from_pretrained(
        args.directory, local_files_only=True, dtype=torch.float32
    ).eval()
    image = image_at(128, 96)
    if args.video:
        frames = [image, image] if args.two_frames else [image]
        inputs = processor(videos=[frames], return_tensors="pt")
    else:
        inputs = processor(images=image, return_tensors="pt")
    with torch.no_grad():
        soft_tokens = (
            model.get_video_features(
                inputs["pixel_values_videos"],
                inputs["video_position_ids"],
                inputs["num_frames_per_video"],
                return_dict=True,
            ).pooler_output[0]
            if args.video and not args.two_frames
            else None
        )
        states = model(**inputs).last_hidden_state
        vector = F.normalize(states.mean(dim=1), dim=-1)[0]
    args.output.write_text(
        json.dumps(
            {
                "revision": "914f7f89142e33e77833254d9c9b90c3cef7303b",
                "ids": inputs["input_ids"][0].tolist(),
                "vector": vector.tolist(),
                "soft_tokens": soft_tokens.tolist() if soft_tokens is not None else None,
            },
            separators=(",", ":"),
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
