#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["rapidfuzz"]
# ///
"""Scores heading, footnote and citation-marker handling against `structure_gold.json`.

    uv run structure_score.py --dump /tmp/dump/text-layer [--gold structure_gold.json] [-v]

Headings: a unit of kind Heading matches a labelled heading when their normalized text is >= 0.85
similar. Precision and recall are over the labelled pages only (every heading on them is listed).
Footnotes: labelled footnote starts must appear as units under a section ending in `Footnotes`,
and no unit under `Footnotes` may be absent from the labels. Markers: labelled raised markers
(`word`, `marker`) must no longer appear glued (`word`+`marker`) in any unit of the page.
"""

from __future__ import annotations

import argparse
import json
import re
import unicodedata
from pathlib import Path

from rapidfuzz import fuzz


def norm(text: str) -> str:
    text = unicodedata.normalize("NFKC", text).lower()
    return re.sub(r"\s+", " ", re.sub(r"[^\w\s]", "", text)).strip()


def squash(text: str) -> str:
    return text.translate(str.maketrans({"\u201d": "'", "\u201c": "'", "\u2019": "'", "\u2018": "'", '"': "'"}))


def load(path: Path) -> dict[int, list[dict]]:
    pages: dict[int, list[dict]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            record = json.loads(line)
            pages.setdefault(record["page"], []).append(record)
    return pages


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump", type=Path, required=True, help="folder of <pdf>.jsonl files")
    ap.add_argument("--gold", type=Path, default=Path(__file__).with_name("structure_gold.json"))
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    gold = json.loads(args.gold.read_text(encoding="utf-8"))["pages"]
    tp = fp = fn = 0
    note_tp = note_fp = note_fn = 0
    leaked = markers = 0
    for entry in gold:
        path = args.dump / f"{entry['pdf']}.jsonl"
        if not path.exists():
            continue
        units = load(path).get(entry["page"], [])
        found = [u["text"] for u in units if u["kind"] == "Heading"]
        wanted = entry.get("headings", [])
        used: set[int] = set()
        for heading in wanted:
            hit = next(
                (i for i, f in enumerate(found) if i not in used
                 and fuzz.ratio(norm(f), norm(heading)) >= 85),
                None,
            )
            if hit is None:
                fn += 1
                if args.verbose:
                    print(f"  MISS  {entry['pdf'][:28]} p{entry['page']}: {heading}")
            else:
                used.add(hit)
                tp += 1
        for i, text in enumerate(found):
            if i not in used:
                fp += 1
                if args.verbose:
                    print(f"  EXTRA {entry['pdf'][:28]} p{entry['page']}: {text[:80]}")
        if "footnotes" in entry:
            notes = [u["text"] for u in units if u.get("section", "").endswith("Footnotes")]
            used_notes: set[int] = set()
            for start in entry["footnotes"]:
                hit = next((i for i, n in enumerate(notes) if i not in used_notes
                            and norm(n).startswith(norm(start))), None)
                if hit is None:
                    note_fn += 1
                    if args.verbose:
                        print(f"  NOTE-MISS {entry['pdf'][:28]} p{entry['page']}: {start}")
                else:
                    used_notes.add(hit)
                    note_tp += 1
            for i, n in enumerate(notes):
                # A note that follows a matched one without a number of its own is its wrapped
                # remainder, not a separate footnote.
                if i not in used_notes and not (i - 1 in used_notes and not re.match(r"^\W*\d|^[*†‡∗§]", n.strip())):
                    note_fp += 1
                    if args.verbose:
                        print(f"  NOTE-EXTRA {entry['pdf'][:28]} p{entry['page']}: {n[:80]}")
        for word, marker in entry.get("markers", []):
            markers += 1
            pattern = re.compile(re.escape(squash(word)) + r"\s?" + re.escape(marker) + r"(?!\d)")
            if any(pattern.search(squash(u["text"])) for u in units):
                leaked += 1
                if args.verbose:
                    print(f"  LEAK  {entry['pdf'][:28]} p{entry['page']}: {word}{marker}")
    def ratio(a: int, b: int) -> float:
        return a / b if b else float("nan")
    p, r = ratio(tp, tp + fp), ratio(tp, tp + fn)
    print(f"headings: P {p:.3f} R {r:.3f} F1 {2*p*r/(p+r) if p+r else 0:.3f}  (tp {tp} fp {fp} fn {fn})")
    if note_tp + note_fp + note_fn:
        p, r = ratio(note_tp, note_tp + note_fp), ratio(note_tp, note_tp + note_fn)
        print(f"footnotes: P {p:.3f} R {r:.3f}  (tp {note_tp} fp {note_fp} fn {note_fn})")
    if markers:
        print(f"markers leaked into prose: {leaked}/{markers}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
