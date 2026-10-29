#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""Transcribes rendered pages (and table crops) with a MiniCPM-OCR llama-server.

Start the server first (llama.cpp b11826 or newer), for example:

    llama-server -m MiniCPM-OCR-1.0-AD-Q5_K_M.gguf --mmproj mmproj-MiniCPM-OCR-1.0-Q8_0.gguf \
      --jinja -c 16384 -ngl 99

then:

    uv run transcribe.py --pages /tmp/pages --out /tmp/gold --tables tables.json

Pages: `Text Recognition:` -> `<out>/<pdf>/p-0007.txt`.
Tables (optional manifest, see tables.example.json): the page image is cropped to the box and sent
with `Table Recognition:` -> `<out>/tables/<id>.html`, plus the crop as `<id>.png` for checking.
Existing outputs are kept, so reruns resume.
"""

from __future__ import annotations

import argparse
import base64
import io
import json
import sys
import time
import urllib.request
from pathlib import Path

from PIL import Image


def ask(
    server: str, png: bytes, prompt: str, max_tokens: int, timeout: float
) -> tuple[str, dict]:
    """One image + one prompt, greedy decoding, as the model card prescribes."""
    image = base64.b64encode(png).decode("ascii")
    payload = {
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {"url": f"data:image/png;base64,{image}"},
                    },
                    {"type": "text", "text": prompt},
                ],
            }
        ],
        "temperature": 0,
        "max_tokens": max_tokens,
    }
    request = urllib.request.Request(
        f"{server.rstrip('/')}/v1/chat/completions",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
    )
    started = time.monotonic()
    with urllib.request.urlopen(request, timeout=timeout) as response:
        body = json.load(response)
    choice = body["choices"][0]
    text = choice["message"].get("content") or ""
    meta = {
        "seconds": round(time.monotonic() - started, 1),
        "finish_reason": choice.get("finish_reason"),
        "usage": body.get("usage", {}),
    }
    return text, meta


def crop(png_path: Path, box: list[float]) -> bytes:
    """Crops by fractions of the page: [left, top, right, bottom] in 0..1."""
    with Image.open(png_path) as page:
        width, height = page.size
        region = page.crop(
            (
                round(box[0] * width),
                round(box[1] * height),
                round(box[2] * width),
                round(box[3] * height),
            )
        )
        buffer = io.BytesIO()
        region.save(buffer, format="PNG")
        return buffer.getvalue()


def save(text: str, meta: dict, target: Path) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")
    target.with_suffix(target.suffix + ".meta.json").write_text(
        json.dumps(meta), encoding="utf-8"
    )
    if meta["finish_reason"] == "length":
        print(
            f"  warning: {target.name} hit the token limit (repetition loop?)",
            file=sys.stderr,
        )


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument(
        "--pages", type=Path, required=True, help="folder from render_pages.py"
    )
    parser.add_argument("--out", type=Path, required=True, help="gold output folder")
    parser.add_argument("--server", default="http://127.0.0.1:8080")
    parser.add_argument("--tables", type=Path, help="table manifest (JSON list)")
    parser.add_argument("--max-tokens", type=int, default=8192)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--only", help="only PDFs whose name contains this text")
    args = parser.parse_args()

    for folder in sorted(path for path in args.pages.iterdir() if path.is_dir()):
        if args.only and args.only not in folder.name:
            continue
        for image in sorted(folder.glob("p-*.png")):
            target = args.out / folder.name / f"{image.stem}.txt"
            if target.exists():
                continue
            print(f"{folder.name}/{image.name}")
            text, meta = ask(
                args.server,
                image.read_bytes(),
                "Text Recognition:",
                args.max_tokens,
                args.timeout,
            )
            save(text, meta, target)

    if args.tables is not None:
        for entry in json.loads(args.tables.read_text(encoding="utf-8")):
            target = args.out / "tables" / f"{entry['id']}.html"
            if target.exists():
                continue
            source = args.pages / entry["pdf"] / f"p-{entry['page']:04d}.png"
            print(f"table {entry['id']} ({source.parent.name} p{entry['page']})")
            if not source.exists():
                sys.exit(
                    f"{source} is missing: run render_pages.py with --tables {args.tables}"
                )
            png = crop(source, entry["box"])
            (args.out / "tables").mkdir(parents=True, exist_ok=True)
            (args.out / "tables" / f"{entry['id']}.png").write_bytes(png)
            text, meta = ask(
                args.server, png, "Table Recognition:", args.max_tokens, args.timeout
            )
            save(text, meta, target)
    return 0


if __name__ == "__main__":
    sys.exit(main())
