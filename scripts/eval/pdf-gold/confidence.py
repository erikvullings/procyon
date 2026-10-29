# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Does the per-page layout confidence predict how well a page was read?

Joins the confidence dump (`dump_confidence`, `<dump>/confidence/<pdf>.jsonl`) with the per-page
scores of `score.py --page-scores` for one method, prints the rank correlation with the gold
reading order and the mean reading order by confidence band. A useful score is monotone: low
confidence should mean low reading order, and the bottom band is the set worth a second opinion.
"""

import argparse
import json
from pathlib import Path


def ranks(values: list[float]) -> list[float]:
    order = sorted(range(len(values)), key=lambda i: values[i])
    result = [0.0] * len(values)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and values[order[j + 1]] == values[order[i]]:
            j += 1
        for k in range(i, j + 1):
            result[order[k]] = (i + j) / 2 + 1
        i = j + 1
    return result


def spearman(a: list[float], b: list[float]) -> float:
    ra, rb = ranks(a), ranks(b)
    n = len(a)
    ma, mb = sum(ra) / n, sum(rb) / n
    cov = sum((x - ma) * (y - mb) for x, y in zip(ra, rb))
    va = sum((x - ma) ** 2 for x in ra) ** 0.5
    vb = sum((y - mb) ** 2 for y in rb) ** 0.5
    return cov / (va * vb) if va and vb else 0.0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--page-scores", type=Path, required=True)
    parser.add_argument("--method", default="text-layer")
    parser.add_argument(
        "--min-words-f1",
        type=float,
        default=0.5,
        help="skip pages whose words F1 is lower: the gold and the output disagree on the content "
        "itself (rotated pages, model errors), so their reading order says nothing",
    )
    args = parser.parse_args()

    confidence: dict[tuple[str, int], float] = {}
    for file in (args.dump / "confidence").glob("*.jsonl"):
        for line in file.read_text().splitlines():
            row = json.loads(line)
            confidence[(file.stem, row["page"])] = row["score"]
    pairs = []
    for line in args.page_scores.read_text().splitlines():
        row = json.loads(line)
        key = (row["pdf"], row["page"])
        if (
            row["method"] == args.method
            and key in confidence
            and row["words_f1"] >= args.min_words_f1
        ):
            pairs.append((confidence[key], row["order"]))
    if len(pairs) < 3:
        print("not enough joined pages")
        return 1
    scores, orders = [p[0] for p in pairs], [p[1] for p in pairs]
    print(
        f"pages: {len(pairs)}  spearman(confidence, reading order): {spearman(scores, orders):.3f}"
    )
    print("\n| confidence | pages | mean reading order | share of pages under 0.5 |")
    print("|---|---|---|---|")
    for low, high in [(0, 0.6), (0.6, 0.8), (0.8, 0.9), (0.9, 0.97), (0.97, 1.01)]:
        band = [o for s, o in pairs if low <= s < high]
        if band:
            under = sum(o < 0.5 for o in band) / len(band)
            print(
                f"| {low:.2f}-{min(high, 1):.2f} | {len(band)} | {sum(band) / len(band):.3f} | {under:.2f} |"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
