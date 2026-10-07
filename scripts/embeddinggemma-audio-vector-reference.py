"""Generate a complete PCM-to-embedding reference against the pinned checkpoint."""

import argparse
import json
from pathlib import Path

import numpy as np
import torch
import torch.nn.functional as F
from transformers import AutoModel, AutoProcessor


def pcm():
    t = np.arange(3200, dtype=np.float32) / 16000
    return (0.3 * np.sin(2 * np.pi * 440 * t) + 0.1 * np.sin(2 * np.pi * 880 * t)).astype(
        np.float32
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    torch.set_num_threads(4)
    processor = AutoProcessor.from_pretrained(args.directory, local_files_only=True)
    model = AutoModel.from_pretrained(
        args.directory, local_files_only=True, dtype=torch.float32
    ).eval()
    waveform = pcm()
    inputs = processor(audio=waveform, sampling_rate=16000, return_tensors="pt")
    quantized = np.rint(waveform * np.float32(32767)).astype(np.int16)
    decoded = quantized.astype(np.float32) / np.float32(32768)
    with torch.no_grad():
        states = model(**inputs).last_hidden_state
        vector = F.normalize(states.mean(dim=1), dim=-1)[0]
        decoded_inputs = processor(audio=decoded, sampling_rate=16000, return_tensors="pt")
        decoded_states = model(**decoded_inputs).last_hidden_state
        decoded_vector = F.normalize(decoded_states.mean(dim=1), dim=-1)[0]
    args.output.write_text(
        json.dumps(
            {
                "revision": "914f7f89142e33e77833254d9c9b90c3cef7303b",
                "ids": inputs["input_ids"][0].tolist(),
                "samples": len(waveform),
                "vector": vector.tolist(),
                "pcm16_vector": decoded_vector.tolist(),
            },
            separators=(",", ":"),
        )
        + "\n"
    )


if __name__ == "__main__":
    main()
