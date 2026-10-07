"""Offline Gemma4 16-kHz feature golden from the pinned Transformers processor."""

import json
import math
import sys
from pathlib import Path

import numpy as np
from transformers.models.gemma4.feature_extraction_gemma4 import Gemma4AudioFeatureExtractor

REVISION = "914f7f89142e33e77833254d9c9b90c3cef7303b"


def waveform(length: int) -> np.ndarray:
    return np.asarray(
        [
            0.35 * math.sin(2 * math.pi * 440 * i / 16000)
            + 0.12 * math.cos(2 * math.pi * 820 * i / 16000)
            + 0.05 * math.sin(2 * math.pi * i * i / 320000)
            for i in range(length)
        ],
        dtype=np.float32,
    )


def main(model_dir: Path, output: Path) -> None:
    assert model_dir.name == REVISION
    extractor = Gemma4AudioFeatureExtractor.from_pretrained(model_dir, local_files_only=True)
    sounds = [waveform(3200), waveform(500)]
    reference = extractor(sounds)
    cases = []
    for sound, features, mask in zip(
        sounds, reference["input_features"], reference["input_features_mask"]
    ):
        cases.append(
            {
                "pcm": sound.tolist(),
                "padded_samples": 3200,
                "frame_count": len(features),
                "valid_frames": int(mask.sum()),
                "mask": mask.tolist(),
                "features": features.flatten().tolist(),
            }
        )
    output.write_text(json.dumps({"revision": REVISION, "cases": cases}) + "\n")


if __name__ == "__main__":
    main(Path(sys.argv[1]).resolve(), Path(sys.argv[2]))
