#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Renders PDF pages to PNG with pdftoppm, one folder per PDF.

    uv run render_pages.py --pdf-dir ~/Downloads/corpus --out /tmp/pages --dpi 150 --sample 6

Pages are named `p-0007.png` (1-based). Existing images are kept, so reruns resume.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path


def page_count(pdf: Path) -> int:
    info = subprocess.run(
        ["pdfinfo", str(pdf)], capture_output=True, text=True, check=True
    ).stdout
    match = re.search(r"^Pages:\s+(\d+)", info, re.MULTILINE)
    if match is None:
        raise RuntimeError(f"pdfinfo reported no page count for {pdf}")
    return int(match.group(1))


def sampled_pages(total: int, sample: int | None) -> list[int]:
    """Evenly spaced pages, always including the first; all pages when `sample` is unset."""
    if sample is None or sample >= total:
        return list(range(1, total + 1))
    if sample <= 1:
        return [1]
    step = (total - 1) / (sample - 1)
    return sorted({1 + round(index * step) for index in range(sample)})


def render(pdf: Path, out: Path, dpi: int, pages: list[int]) -> int:
    folder = out / pdf.stem
    folder.mkdir(parents=True, exist_ok=True)
    rendered = 0
    for page in pages:
        target = folder / f"p-{page:04d}.png"
        if target.exists():
            continue
        prefix = folder / f"p-{page:04d}"
        subprocess.run(
            [
                "pdftoppm",
                "-r",
                str(dpi),
                "-png",
                "-singlefile",
                "-f",
                str(page),
                "-l",
                str(page),
                str(pdf),
                str(prefix),
            ],
            check=True,
        )
        rendered += 1
    return rendered


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--dpi", type=int, default=150)
    parser.add_argument("--sample", type=int, help="pages per PDF (default: all)")
    parser.add_argument(
        "--tables",
        type=Path,
        help="table manifest: its pages are rendered in addition to the sample",
    )
    args = parser.parse_args()

    wanted: dict[str, set[int]] = {}
    if args.tables is not None:
        for entry in json.loads(args.tables.read_text(encoding="utf-8")):
            wanted.setdefault(entry["pdf"], set()).add(entry["page"])

    for pdf in sorted(args.pdf_dir.glob("*.pdf")):
        pages = sorted(
            set(sampled_pages(page_count(pdf), args.sample))
            | wanted.get(pdf.stem, set())
        )
        count = render(pdf, args.out, args.dpi, pages)
        print(f"{pdf.stem}: {len(pages)} pages ({count} new)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
