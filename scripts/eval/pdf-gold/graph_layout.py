#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pymupdf"]
# ///
"""Experiment (task 0295): multi-relational document graph for PDF layout, scored with score.py.

    uv run graph_layout.py --pdf-dir ~/Downloads/"Custom Chat Leiden" --out /tmp/dump

Writes `<out>/graph-<config>/<pdf>.jsonl` in the format of the Rust `corpus_dump` test, one
folder per ablation:

  A  spatial edges only: glyph nearest-neighbour graph, word/line/paragraph links by geometry
  B  A + typographic edges: font size / weight / family compatibility gates paragraph links
  C  B + whitespace edges: vertical corridors (cut) and the page's own line pitch (break)

Levels: glyph graph (next-glyph + superscript edges) -> words -> lines -> paragraphs -> reading
order (bands cut by page-wide paragraphs, columns inside a band). Not covered: tables.
"""

from __future__ import annotations

import argparse
import json
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from statistics import median

import pymupdf as fitz

CONFIGS = {
    "A": {"typo": False, "white": False},
    "S": {"typo": "size", "white": False},
    "B": {"typo": True, "white": False},
    "C": {"typo": True, "white": True},
}


# ------------------------------------------------------------------ nodes


@dataclass
class Glyph:
    ch: str
    x0: float
    x1: float
    y0: float
    y1: float
    base: float
    size: float
    font: str
    bold: bool
    space: bool


@dataclass
class Word:
    glyphs: list[Glyph]
    text: str = ""
    x0: float = 0.0
    x1: float = 0.0
    y0: float = 0.0
    y1: float = 0.0
    base: float = 0.0
    size: float = 0.0
    font: str = ""
    bold: bool = False
    sup: int = 0  # glyphs attached as super/subscripts


@dataclass
class Line:
    words: list[Word]
    text: str = ""
    x0: float = 0.0
    x1: float = 0.0
    y0: float = 0.0
    y1: float = 0.0
    base: float = 0.0
    size: float = 0.0
    font: str = ""
    bold: bool = False
    page: int = 0


@dataclass
class Para:
    lines: list[Line] = field(default_factory=list)

    @property
    def x0(self) -> float:
        return min(l.x0 for l in self.lines)

    @property
    def x1(self) -> float:
        return max(l.x1 for l in self.lines)

    @property
    def top(self) -> float:
        return min(l.y0 for l in self.lines)

    @property
    def bottom(self) -> float:
        return max(l.y1 for l in self.lines)


class DisjointSet:
    def __init__(self, n: int) -> None:
        self.parent = list(range(n))

    def find(self, a: int) -> int:
        while self.parent[a] != a:
            self.parent[a] = self.parent[self.parent[a]]
            a = self.parent[a]
        return a

    def union(self, a: int, b: int) -> None:
        self.parent[self.find(a)] = self.find(b)

    def groups(self) -> list[list[int]]:
        out: dict[int, list[int]] = defaultdict(list)
        for i in range(len(self.parent)):
            out[self.find(i)].append(i)
        return list(out.values())


def norm_font(name: str) -> str:
    """Drops the subset prefix (`ABCDEF+Times-Bold` -> `times`) and style words."""
    name = re.sub(r"^[A-Z]{6}\+", "", name).lower()
    return re.sub(r"[-,_ ]?(bold|italic|oblique|roman|regular|bd|it|mt|psmt)\b.*$", "", name)


# ------------------------------------------------------------------ level 1: glyph graph


def page_glyphs(page: fitz.Page) -> list[Glyph]:
    glyphs: list[Glyph] = []
    for block in page.get_text("rawdict")["blocks"]:
        if block.get("type") != 0:
            continue
        for line in block["lines"]:
            if abs(line["dir"][1]) > 0.1:  # rotated text is out of scope
                continue
            for span in line["spans"]:
                font = norm_font(span["font"])
                bold = bool(span["flags"] & 16) or "bold" in span["font"].lower()
                for ch in span["chars"]:
                    x0, y0, x1, y1 = ch["bbox"]
                    glyphs.append(
                        Glyph(
                            ch["c"], x0, x1, y0, y1, ch["origin"][1], span["size"],
                            font, bold, ch["c"].isspace(),
                        )
                    )
    return glyphs


def glyph_words(glyphs: list[Glyph]) -> tuple[list[Word], int]:
    """Next-glyph edges (end of one glyph to start of the next on one baseline) and
    super/subscript edges (smaller glyph, raised or lowered baseline); words are components."""
    n = len(glyphs)
    dsu = DisjointSet(n)
    sup_edges = 0
    cell = 24.0
    grid: dict[tuple[int, int], list[int]] = defaultdict(list)
    for i, g in enumerate(glyphs):
        grid[(int(g.x0 // cell), int(g.base // cell))].append(i)
    attached_sup: set[int] = set()
    for i, g in enumerate(glyphs):
        if g.space:
            continue
        cx, cy = int(g.x1 // cell), int(g.base // cell)
        best, best_gap = None, 1e9
        for dx in (0, 1):
            for dy in (-1, 0, 1):
                for j in grid.get((cx + dx, cy + dy), ()):
                    h = glyphs[j]
                    if j == i or h.x0 < g.x0:
                        continue
                    gap = h.x0 - g.x1
                    if gap < -0.5 * g.size or gap >= best_gap:
                        continue
                    same_base = abs(h.base - g.base) <= 0.25 * max(g.size, h.size)
                    small, large = (h, g) if h.size < g.size else (g, h)
                    is_sup = (
                        small.size <= 0.85 * large.size
                        and 0.15 * large.size < abs(h.base - g.base) < 0.8 * large.size
                    )
                    if not (same_base or is_sup):
                        continue
                    if gap > 0.2 * max(g.x1 - g.x0, g.size, h.size) + 0.01:
                        continue
                    best, best_gap = j, gap
                    if is_sup and not same_base:
                        best_gap = gap  # remember kind below
        if best is not None:
            h = glyphs[best]
            if h.space:
                continue
            raised = abs(h.base - g.base) > 0.25 * max(g.size, h.size)
            if raised and g.size < h.size:
                continue  # a marker attaches to the host on its left, never onward
            dsu.union(i, best)
            if raised:
                sup_edges += 1
                attached_sup.add(best)
    words: list[Word] = []
    for group in dsu.groups():
        members = sorted((glyphs[k] for k in group if not glyphs[k].space), key=lambda g: g.x0)
        if not members:
            continue
        # The word's own size and baseline come from its dominant (largest) glyphs.
        top = max(g.size for g in members)
        main = [g for g in members if g.size > 0.85 * top]
        w = Word(members)
        base = median(g.base for g in main)
        text, mode = "", ""
        for g in members:
            small = g.size <= 0.85 * top
            now = ("^" if g.base < base - 0.15 * top else "_") if small else ""
            if now != mode:
                text += now
                mode = now
            text += g.ch
        w.text = text
        w.x0, w.x1 = min(g.x0 for g in members), max(g.x1 for g in members)
        w.y0, w.y1 = min(g.y0 for g in main), max(g.y1 for g in main)
        w.base = median(g.base for g in main)
        w.size = top
        w.font = Counter(g.font for g in main).most_common(1)[0][0]
        w.bold = sum(g.bold for g in main) * 2 > len(main)
        w.sup = len(members) - len(main)
        words.append(w)
    return words, sup_edges


# ------------------------------------------------------------------ level 2: lines


def corridor_free(words: list[Word], x0: float, x1: float, base: float, size: float) -> int:
    """How many text rows above and below `base` also leave `x0..x1` empty (a vertical corridor)."""
    rows: dict[int, bool] = {}
    x0, x1 = x0 + 1.5, x1 - 1.5  # tolerate box rounding at the corridor walls
    for w in words:
        if w.x1 <= x0 or w.x0 >= x1:
            continue
        if abs(w.base - base) < 0.5 * size:
            continue
        rows[round(w.base / (0.6 * size))] = True
    # Count rows with text elsewhere on the page near this baseline but empty in the gap.
    return rows_with_text_but_empty(words, rows, base, size)


def rows_with_text_but_empty(words, blocked, base, size) -> int:
    near = {
        round(w.base / (0.6 * size))
        for w in words
        if 0 < abs(w.base - base) <= 6 * 1.4 * size
    }
    empty = len([r for r in near if r not in blocked])
    return empty if empty >= 0.85 * len(near) else 0


def build_lines(words: list[Word], page_no: int, white: bool) -> list[Line]:
    words = sorted(words, key=lambda w: (round(w.base), w.x0))
    n = len(words)
    dsu = DisjointSet(n)
    by_row: dict[int, list[int]] = defaultdict(list)
    for i, w in enumerate(words):
        by_row[round(w.base / 3)].append(i)
    for i, w in enumerate(words):
        best, best_gap = None, 1e9
        for r in (round(w.base / 3) - 1, round(w.base / 3), round(w.base / 3) + 1):
            for j in by_row.get(r, ()):
                v = words[j]
                gap = v.x0 - w.x1
                if j == i or gap < -0.3 * w.size or gap >= best_gap:
                    continue
                if abs(v.base - w.base) > 0.35 * min(v.size, w.size):
                    continue
                best, best_gap = j, gap
        if best is None:
            continue
        v = words[best]
        size = max(w.size, v.size)
        if white:
            if best_gap > 3.0 * size:
                continue
            if best_gap > 0.9 * size:
                # a wide gap that continues as a vertical corridor is a column cut
                if corridor_free(words, w.x1, v.x0, w.base, size) >= 4:
                    continue
        elif best_gap > 1.0 * size:
            continue
        dsu.union(i, best)
    lines: list[Line] = []
    for group in dsu.groups():
        ws = sorted((words[k] for k in group), key=lambda w: w.x0)
        text = ws[0].text
        for prev, cur in zip(ws, ws[1:]):
            text += (" " if cur.x0 - prev.x1 > 0.12 * cur.size else "") + cur.text
        top = max(w.size for w in ws)
        main = [w for w in ws if w.size > 0.85 * top] or ws
        ln = Line(ws, text, min(w.x0 for w in ws), max(w.x1 for w in ws),
                  min(w.y0 for w in main), max(w.y1 for w in main),
                  median(w.base for w in main), top,
                  Counter(w.font for w in main).most_common(1)[0][0],
                  sum(w.bold for w in main) * 2 > len(main), page_no)
        lines.append(ln)
    return lines


# ------------------------------------------------------------------ level 3: paragraphs

BULLET = re.compile(r"^(\[\d+\]|\(?\d+[.)]|[•▪■·–-]\s|\(?[a-z][.)]\s)")


def build_paragraphs(lines: list[Line], typo: bool, white: bool) -> list[Para]:
    lines = sorted(lines, key=lambda l: (l.base, l.x0))
    # line pitch of the page (modal text pitch) for the whitespace test
    pitches = []
    for l in lines:
        below = [m for m in lines if m.base > l.base + 0.5 * l.size and overlap(l, m) > 0.3]
        if below:
            pitches.append(min(m.base for m in below) - l.base)
    pitch = median(pitches) if pitches else 12.0
    pitch = Counter(round(p, 1) for p in pitches if 0.7 * pitch < p < 1.5 * pitch).most_common(1)[0][0] if pitches else pitch
    body_left = Counter(round(l.x0) for l in lines).most_common(1)[0][0] if lines else 0
    nxt: dict[int, int] = {}
    for i, l in enumerate(lines):
        cands = [
            (m.base, j) for j, m in enumerate(lines)
            if m.base > l.base + 0.5 * l.size and overlap(l, m) > 0.3
        ]
        if cands:
            nxt[i] = min(cands)[1]
    dsu = DisjointSet(len(lines))
    for i, j in nxt.items():
        a, b = lines[i], lines[j]
        # spatial: vertical distance and horizontal indent
        dist = b.base - a.base
        if dist > (1.65 if not white else 1.45) * max(pitch, a.size * 1.1):
            continue
        if b.x0 - a.x0 > 0.9 * a.size:  # first-line indent
            continue
        if BULLET.match(b.text) and abs(b.x0 - a.x0) < 3 * a.size and (typo or white):
            continue
        if typo:
            if abs(a.size - b.size) > 0.12 * max(a.size, b.size):
                continue
            if typo != "size" and (a.bold != b.bold or a.font != b.font and not (a.bold or b.bold)):
                continue
        if white:
            # a short line closes its paragraph when the next one starts at the left margin
            width = max(l.x1 for l in lines if overlap(a, l) > 0.3) - min(
                l.x0 for l in lines if overlap(a, l) > 0.3
            )
            if a.x1 < a.x0 + 0.6 * width and b.x0 <= a.x0 + 0.5 * a.size and a.x0 - 0 <= body_left + 2 * a.size:
                if width > 8 * a.size:
                    continue
        dsu.union(i, j)
    paras = []
    for group in dsu.groups():
        paras.append(Para(sorted((lines[k] for k in group), key=lambda l: l.base)))
    return paras


def overlap(a, b) -> float:
    inter = min(a.x1, b.x1) - max(a.x0, b.x0)
    return max(0.0, inter) / max(1.0, min(a.x1 - a.x0, b.x1 - b.x0))


# ------------------------------------------------------------------ level 4: reading order


def reading_order(paras: list[Para], page_width: float) -> list[Para]:
    if not paras:
        return []
    left = min(p.x0 for p in paras)
    right = max(p.x1 for p in paras)
    wide_limit = 0.7 * (right - left)
    wide = sorted((p for p in paras if p.x1 - p.x0 > wide_limit), key=lambda p: p.top)
    rest = [p for p in paras if p not in wide]
    order: list[Para] = []
    edges = [-1e9] + [p.top for p in wide] + [1e9]
    bands: list[list[Para]] = [[] for _ in range(len(edges) - 1)]
    for p in rest:
        for k in range(len(edges) - 1):
            if edges[k] <= p.top < edges[k + 1]:
                bands[k].append(p)
                break
    for k, band in enumerate(bands):
        cols: list[list[Para]] = []
        for p in sorted(band, key=lambda p: p.x0):
            for c in cols:
                cx0, cx1 = min(q.x0 for q in c), max(q.x1 for q in c)
                inter = min(cx1, p.x1) - max(cx0, p.x0)
                if inter > 0.5 * (p.x1 - p.x0):
                    c.append(p)
                    break
            else:
                cols.append([p])
        cols.sort(key=lambda c: min(q.x0 for q in c))
        for c in cols:
            order.extend(sorted(c, key=lambda q: q.top))
        if k < len(wide):
            order.append(wide[k])
    return order


# ------------------------------------------------------------------ document


def para_text(p: Para) -> str:
    text = ""
    for l in p.lines:
        t = l.text.strip()
        if text.endswith("-") and t[:1].islower() and len(text) > 1 and text[-2].isalpha():
            text = text[:-1] + t
        else:
            text = (text + " " + t) if text else t
    return text


def furniture_keys(pages: list[list[Line]], heights: list[float]) -> set[str]:
    seen: dict[str, set[int]] = defaultdict(set)
    for n, lines in enumerate(pages):
        for l in lines:
            if l.y0 < 0.08 * heights[n] or l.y1 > 0.93 * heights[n]:
                seen[re.sub(r"\d+", "#", l.text.strip().lower())].add(n)
    return {k for k, v in seen.items() if len(v) >= 3 or re.fullmatch(r"#+", k)}


def convert(pdf: Path, cfg: dict) -> tuple[list[dict], dict]:
    doc = fitz.open(pdf)
    per_page_lines: list[list[Line]] = []
    heights, widths, stats = [], [], Counter()
    for n, page in enumerate(doc):
        glyphs = page_glyphs(page)
        words, sup = glyph_words(glyphs)
        stats["glyphs"] += len(glyphs)
        stats["sup_edges"] += sup
        stats["words"] += len(words)
        per_page_lines.append(build_lines(words, n + 1, cfg["white"]))
        heights.append(page.rect.height)
        widths.append(page.rect.width)
    drop = furniture_keys(per_page_lines, heights)
    records: list[dict] = []
    for n, lines in enumerate(per_page_lines):
        lines = [
            l for l in lines
            if not (
                (l.y0 < 0.08 * heights[n] or l.y1 > 0.93 * heights[n])
                and re.sub(r"\d+", "#", l.text.strip().lower()) in drop
            )
        ]
        paras = build_paragraphs(lines, cfg["typo"], cfg["white"])
        for k, p in enumerate(reading_order(paras, widths[n])):
            text = para_text(p)
            if text.strip():
                records.append({"page": n + 1, "block": k, "kind": "Paragraph", "text": text})
                stats["paragraphs"] += 1
    return records, dict(stats)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pdf-dir", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--configs", default="ABC")
    ap.add_argument("--only")
    args = ap.parse_args()
    for pdf in sorted(args.pdf_dir.glob("*.pdf")):
        if args.only and args.only not in pdf.name:
            continue
        for name in args.configs:
            records, stats = convert(pdf, CONFIGS[name])
            out = args.out / f"graph-{name}"
            out.mkdir(parents=True, exist_ok=True)
            (out / f"{pdf.stem}.jsonl").write_text(
                "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records), "utf-8"
            )
            print(f"{name} {pdf.stem[:50]:50} {stats}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
