#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["rapidfuzz"]
# ///
"""Scores PDF conversion dumps against a gold standard.

    uv run score.py --gold /tmp/gold --dump /tmp/dump --tables tables.json --verified verified.txt

`--dump` holds one folder per method (`text-layer/`, `ml/`, ...) of `<pdf>.jsonl` files written by
`cargo test -p fm-semantic-docling --test corpus_dump -- --ignored`. `--gold` holds the MiniCPM
transcriptions (`<pdf>/p-0007.txt`, `tables/<id>.html`); a hand-corrected page or table can be
placed next to it as `p-0007.verified.txt` / `<id>.verified.html` and wins over the model output.

Per page (macro-averaged):
  words F1      bag-of-words overlap: did the content survive at all, ignoring order
  reading order word-sequence similarity (1 - normalized Levenshtein): is it in the right order
  paragraph P/R how well unit boundaries match gold paragraph boundaries (first 4 words)
  units/gold    unit count over gold paragraph count (1.0 is ideal, >1 means fragmentation)
Per table: cell F1 (multiset of normalized cells) and rows found (fuzzy >= 0.9).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import unicodedata
from collections import Counter
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from statistics import mean

from rapidfuzz import fuzz
from rapidfuzz.distance import Levenshtein

DASHES = str.maketrans({c: "-" for c in "‐‑‒–—−"})
WORD = re.compile(r"[^\W_]+(?:[.,][^\W_]+)*", re.UNICODE)
START_WORDS = 4


def clean(text: str) -> str:
    text = unicodedata.normalize("NFKC", text).translate(DASHES)
    return text.replace("­", "").replace("⁎", "*").lower()


def words(text: str) -> list[str]:
    return WORD.findall(clean(text))


class _Html(HTMLParser):
    """Collects table rows as lists of cell texts; other text goes to `loose`."""

    def __init__(self) -> None:
        super().__init__()
        self.rows: list[list[str]] = []
        self.loose: list[str] = []
        self._cell: list[str] | None = None

    def handle_starttag(self, tag: str, attrs: list) -> None:
        if tag == "tr":
            self.rows.append([])
        elif tag in ("td", "th"):
            self._cell = []
            if not self.rows:
                self.rows.append([])

    def handle_endtag(self, tag: str) -> None:
        if tag in ("td", "th") and self._cell is not None:
            self.rows[-1].append(" ".join("".join(self._cell).split()))
            self._cell = None

    def handle_data(self, data: str) -> None:
        (self._cell if self._cell is not None else self.loose).append(data)


OTSL_BLOCK = re.compile(r"<otsl>(.*?)(?:</otsl>|$)", re.DOTALL)
OTSL_TAG = re.compile(r"<(fcel|ecel|lcel|ucel|xcel|ched|rhed|srow|nl)>")


def otsl_rows(block: str) -> list[list[str]]:
    """Rows of cell texts from an OTSL table (`<fcel>text<ecel><nl>...`).

    MiniCPM-OCR answers table pages in OTSL; spanned and empty cells (`ecel`, `lcel`, `ucel`,
    `xcel`) become empty strings so column positions stay aligned.
    """
    rows: list[list[str]] = [[]]
    parts = OTSL_TAG.split(block)  # text, tag, text, tag, ...
    for tag, text in zip(parts[1::2], parts[2::2], strict=False):
        if tag == "nl":
            rows.append([])
        else:
            rows[-1].append(
                " ".join(text.split())
                if tag in ("fcel", "ched", "rhed", "srow")
                else ""
            )
    return [row for row in rows if any(cell for cell in row)]


def otsl_tables(markup: str) -> list[list[list[str]]]:
    return [rows for block in OTSL_BLOCK.findall(markup) if (rows := otsl_rows(block))]


def html_rows(markup: str) -> list[list[str]]:
    if "<otsl>" in markup:
        return [row for table in otsl_tables(markup) for row in table]
    parser = _Html()
    parser.feed(markup)
    return [row for row in parser.rows if any(cell for cell in row)]


def gold_text(markup: str) -> str:
    """Plain text of a gold page: tables flattened row by row, tags dropped."""
    markup = OTSL_BLOCK.sub(
        lambda match: (
            "\n" + "\n".join(" ".join(row) for row in otsl_rows(match.group(1))) + "\n"
        ),
        markup,
    )
    parser = _Html()
    parser.feed(re.sub(r"<br\s*/?>", "\n", markup))
    table_text = "\n".join(" ".join(row) for row in parser.rows)
    return "\n".join(
        part for part in ("".join(parser.loose), table_text) if part.strip()
    )


def gold_paragraphs(text: str) -> list[str]:
    blocks = [block.strip() for block in re.split(r"\n\s*\n", text) if block.strip()]
    if len(blocks) <= 1:  # the model separated lines with single newlines
        blocks = [line.strip() for line in text.splitlines() if line.strip()]
    return blocks


def start_key(text: str) -> tuple[str, ...]:
    return tuple(words(text)[:START_WORDS])


def f1(gold: Counter, candidate: Counter) -> float:
    common = sum((gold & candidate).values())
    if not common:
        return 0.0
    precision, recall = common / sum(candidate.values()), common / sum(gold.values())
    return 2 * precision * recall / (precision + recall)


@dataclass
class PageScore:
    words_f1: float
    order: float
    para_precision: float
    para_recall: float
    units_per_gold: float


def score_page(gold: str, units: list[dict]) -> PageScore:
    gold_words = words(gold)
    text_units = [u for u in units if u["kind"] != "Table"]
    candidate_text = "\n".join(u["text"].replace("\t", " ") for u in units)
    candidate_words = words(candidate_text)
    paragraphs = gold_paragraphs(gold)
    gold_starts = {start_key(p) for p in paragraphs if start_key(p)}
    unit_starts = [start_key(u["text"]) for u in text_units if start_key(u["text"])]
    hits_recall = sum(1 for key in gold_starts if key in set(unit_starts))
    hits_precision = sum(1 for key in unit_starts if key in gold_starts)
    return PageScore(
        words_f1=f1(Counter(gold_words), Counter(candidate_words)),
        order=Levenshtein.normalized_similarity(gold_words, candidate_words),
        para_precision=hits_precision / len(unit_starts) if unit_starts else 0.0,
        para_recall=hits_recall / len(gold_starts) if gold_starts else 0.0,
        units_per_gold=len(units) / len(paragraphs) if paragraphs else 0.0,
    )


def cell_key(cell: str) -> str:
    return re.sub(r"\s+", "", clean(cell))


@dataclass
class TableScore:
    cell_f1: float
    rows_found: float


def score_table(
    gold_rows: list[list[str]], candidate_rows: list[list[str]]
) -> TableScore:
    gold_cells = Counter(cell_key(c) for row in gold_rows for c in row if cell_key(c))
    candidate_cells = Counter(
        cell_key(c) for row in candidate_rows for c in row if cell_key(c)
    )
    candidate_joined = ["".join(cell_key(c) for c in row) for row in candidate_rows]
    found = 0
    for row in gold_rows:
        joined = "".join(cell_key(c) for c in row)
        if joined and any(
            fuzz.ratio(joined, other) >= 90 for other in candidate_joined
        ):
            found += 1
    return TableScore(
        f1(gold_cells, candidate_cells), found / len(gold_rows) if gold_rows else 0.0
    )


def best_candidate(gold_rows: list[list[str]], units: list[dict]) -> list[list[str]]:
    """The Table unit on the page that shares the most cells with the gold table."""
    gold_cells = Counter(cell_key(c) for row in gold_rows for c in row if cell_key(c))
    best: list[list[str]] = []
    best_overlap = -1
    for unit in units:
        if unit["kind"] != "Table":
            continue
        rows = [line.split("\t") for line in unit["text"].splitlines() if line.strip()]
        cells = Counter(cell_key(c) for row in rows for c in row if cell_key(c))
        overlap = sum((gold_cells & cells).values())
        if overlap > best_overlap:
            best, best_overlap = rows, overlap
    return best


def load_dump(path: Path) -> dict[int, list[dict]]:
    pages: dict[int, list[dict]] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            record = json.loads(line)
            pages.setdefault(record["page"], []).append(record)
    return pages


def gold_source(path: Path) -> str:
    verified = path.with_suffix(".verified.txt")
    return (verified if verified.exists() else path).read_text(encoding="utf-8")


@dataclass
class Report:
    pages: dict[str, list[PageScore]] = field(default_factory=dict)
    tables: dict[str, list[TableScore]] = field(default_factory=dict)


def fmt(values: list[float]) -> str:
    return f"{mean(values):.3f}" if values else "n/a"


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawTextHelpFormatter
    )
    parser.add_argument("--gold", type=Path, required=True)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument(
        "--tables", type=Path, help="table manifest (same file transcribe.py used)"
    )
    parser.add_argument(
        "--verified",
        type=Path,
        help="text file listing hand-checked pages as `<pdf>/p-0007` (one per line)",
    )
    parser.add_argument(
        "--report", type=Path, help="also write the Markdown report here"
    )
    args = parser.parse_args()

    verified = set(args.verified.read_text().split()) if args.verified else set()
    methods = sorted(path.name for path in args.dump.iterdir() if path.is_dir())
    pages_all = {m: Report() for m in methods}
    pages_checked = {m: Report() for m in methods}
    auto_tables: dict[str, list[TableScore]] = {m: [] for m in methods}
    empty: dict[str, int] = {}
    empty_docs: dict[str, set[str]] = {}
    per_document: dict[tuple[str, str], list[PageScore]] = {}

    for folder in sorted(
        p for p in args.gold.iterdir() if p.is_dir() and p.name != "tables"
    ):
        for gold_file in sorted(folder.glob("p-*.txt")):
            if gold_file.name.endswith(".verified.txt"):
                continue
            page = int(gold_file.stem.split("-")[1])
            source = gold_source(gold_file)
            gold = gold_text(source)
            if not words(gold):
                continue
            page_tables = [
                t for t in otsl_tables(source) if len(t) >= 3 and max(map(len, t)) >= 2
            ]
            for method in methods:
                dump = args.dump / method / f"{folder.name}.jsonl"
                units = load_dump(dump).get(page, []) if dump.exists() else []
                if not units:
                    # No output for this page (no text layer, or a failed conversion): counted
                    # on its own, not as a zero that would drown the pages that did convert.
                    empty[method] = empty.get(method, 0) + 1
                    empty_docs.setdefault(method, set()).add(folder.name)
                    continue
                result = score_page(gold, units)
                pages_all[method].pages.setdefault("all", []).append(result)
                per_document.setdefault((folder.name, method), []).append(result)
                if f"{folder.name}/{gold_file.stem}" in verified:
                    pages_checked[method].pages.setdefault("all", []).append(result)
                for rows in page_tables:
                    auto_tables[method].append(
                        score_table(rows, best_candidate(rows, units))
                    )

    table_reports = {m: [] for m in methods}
    if args.tables:
        for entry in json.loads(args.tables.read_text(encoding="utf-8")):
            base = args.gold / "tables"
            gold_file = base / f"{entry['id']}.verified.html"
            gold_file = (
                gold_file if gold_file.exists() else base / f"{entry['id']}.html"
            )
            if not gold_file.exists():
                continue
            gold_rows = html_rows(gold_file.read_text(encoding="utf-8"))
            for method in methods:
                dump = args.dump / method / f"{entry['pdf']}.jsonl"
                units = load_dump(dump).get(entry["page"], []) if dump.exists() else []
                table_reports[method].append(
                    score_table(gold_rows, best_candidate(gold_rows, units))
                )

    lines = [
        "## Page text",
        "",
        "| method | pages | words F1 | reading order | para precision | para recall | units/gold |",
        "|---|---|---|---|---|---|---|",
    ]
    for title, reports in (
        ("all pages", pages_all),
        ("hand-verified pages", pages_checked),
    ):
        for method in methods:
            rows = reports[method].pages.get("all", [])
            lines.append(
                f"| {method} ({title}) | {len(rows)} | {fmt([r.words_f1 for r in rows])} | "
                f"{fmt([r.order for r in rows])} | {fmt([r.para_precision for r in rows])} | "
                f"{fmt([r.para_recall for r in rows])} | {fmt([r.units_per_gold for r in rows])} |"
            )
    lines += [
        "",
        "Pages with no output are excluded from the averages above and counted here:",
        "",
    ]
    for method in methods:
        documents = ", ".join(sorted(empty_docs.get(method, set()))) or "none"
        lines.append(f"- {method}: {empty.get(method, 0)} empty pages ({documents})")
    lines += [
        "",
        "## Per document (words F1 / reading order / units per gold paragraph)",
        "",
        "| document | " + " | ".join(methods) + " |",
        "|---|" + "---|" * len(methods),
    ]
    for document in sorted({name for name, _ in per_document}):
        cells = []
        for method in methods:
            rows = per_document.get((document, method), [])
            cells.append(
                f"{fmt([r.words_f1 for r in rows])} / {fmt([r.order for r in rows])} / "
                f"{fmt([r.units_per_gold for r in rows])}"
                if rows
                else "-"
            )
        lines.append(f"| {document[:60]} | " + " | ".join(cells) + " |")
    lines += [
        "",
        "## Tables the model found on whole pages (OTSL, 3+ rows)",
        "",
        "| method | tables | cell F1 | rows found |",
        "|---|---|---|---|",
    ]
    for method in methods:
        rows = auto_tables[method]
        lines.append(
            f"| {method} | {len(rows)} | {fmt([r.cell_f1 for r in rows])} | "
            f"{fmt([r.rows_found for r in rows])} |"
        )
    if args.tables:
        lines += [
            "",
            "## Tables from the manifest",
            "",
            "| method | tables | cell F1 | rows found |",
            "|---|---|---|---|",
        ]
        for method in methods:
            rows = table_reports[method]
            lines.append(
                f"| {method} | {len(rows)} | {fmt([r.cell_f1 for r in rows])} | "
                f"{fmt([r.rows_found for r in rows])} |"
            )
    report = "\n".join(lines) + "\n"
    print(report)
    if args.report:
        args.report.write_text(report, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
