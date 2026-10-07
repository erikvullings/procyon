"""Generate reference embeddings for native text/soft-token fusion from local weights."""

import argparse
import json
from pathlib import Path

import torch
import torch.nn.functional as F
from safetensors import safe_open
from transformers import AutoConfig, AutoModel, AutoTokenizer

REVISION = "914f7f89142e33e77833254d9c9b90c3cef7303b"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()

    torch.set_num_threads(4)
    config = AutoConfig.from_pretrained(args.directory, local_files_only=True)
    config.vision_config = None
    config.audio_config = None
    model = AutoModel.from_pretrained(
        args.directory, config=config, local_files_only=True, dtype=torch.float32
    ).eval()
    tokenizer = AutoTokenizer.from_pretrained(args.directory, local_files_only=True)
    cases = []
    with safe_open(args.directory / "model.safetensors", framework="pt", device="cpu") as weights:
        for modality, text, width in (
            ("vision", "A mountain reflected in a lake", 768),
            ("audio", "Waves breaking against rocks", 1536),
        ):
            token = torch.linspace(-1.0, 1.0, width)
            projected = F.linear(
                token * torch.rsqrt(token.square().mean() + 1e-6),
                weights.get_tensor(f"embed_{modality}.embedding_projection.weight").float(),
            )
            ids = tokenizer(text, return_tensors="pt")["input_ids"][0].tolist()
            position = 2
            ids.insert(position, getattr(model.config, f"{'image' if modality == 'vision' else 'audio'}_token_id"))
            input_ids = torch.tensor([ids])
            input_ids[0, position] = model.config.text_config.pad_token_id
            with torch.no_grad():
                embeds = model.get_input_embeddings()(input_ids)
                embeds[0, position] = projected
                states = model.language_model(inputs_embeds=embeds).last_hidden_state
                vector = F.normalize(states.mean(dim=1), dim=-1)[0]
            cases.append({
                "modality": modality,
                "ids": ids,
                "position": position,
                "soft_token": projected.tolist(),
                "vector": vector.tolist(),
            })

    soft_token = torch.linspace(-0.1, 0.1, 512)
    ids = [config.text_config.bos_token_id]
    for _ in range(2):
        ids.extend([config.boi_token_id, *([config.video_token_id] * 130), config.eoi_token_id])
    ids.append(config.text_config.eos_token_id)
    positions = [i for i, value in enumerate(ids) if value == config.video_token_id]
    input_ids = torch.tensor([ids])
    input_ids[0, positions] = config.text_config.pad_token_id
    with torch.no_grad():
        embeds = model.get_input_embeddings()(input_ids)
        embeds[0, positions] = soft_token
        states = model.language_model(inputs_embeds=embeds).last_hidden_state
        vector = F.normalize(states.mean(dim=1), dim=-1)[0]
    long_video = {"ids": ids, "soft_token": soft_token.tolist(), "vector": vector.tolist()}
    args.output.write_text(
        json.dumps({"revision": REVISION, "cases": cases, "long_video": long_video}, separators=(",", ":"))
        + "\n"
    )


if __name__ == "__main__":
    main()
