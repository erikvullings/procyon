"""Derive bounded human-judged MIRACL dev pools from immutable source snapshots."""

import gzip
import hashlib
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

SOURCE_SHA256 = {
    "sw-corpus.jsonl.gz": "1a604a6571cd74061ba2778082bfff7eddc734c9076e11c5bad04125a39fd246",
    "bn-corpus.jsonl.gz": "7399b221f6dabab5e9ab0eed6107a27091df3807f4684d588357e4c8abbdaef2",
    "sw-qrels.dev.tsv": "a1e6438ac732dabd9a24fd5c8dfc0c64168aea81ff477d8b1986fde5ad193a63",
    "bn-qrels.dev.tsv": "0aab3bcb0bbc5d3e5ed1566a9e3965800239438b5b5f68d5b6bb4acb7f0fec0e",
    "sw-topics.dev.tsv": "15cde3f614cb75b3106fb4606a16162140ee157da22c4a57aae756571788b722",
    "bn-topics.dev.tsv": "46fbf50f1f3972b6908543bad6b6dfc60a75eec80d54aae603efd884ba608d2f",
}
QUERY_COUNT = 50
NEGATIVES_PER_QUERY = 3


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def words(text):
    return set(re.findall(r"\w+", text.casefold()))


def build(root, language):
    topics = {}
    for line in (root / f"{language}-topics.dev.tsv").read_text().splitlines():
        query_id, text = line.split("\t", 1)
        topics[query_id] = text
    judgements = defaultdict(lambda: {0: set(), 1: set()})
    for line in (root / f"{language}-qrels.dev.tsv").read_text().splitlines():
        query_id, unused, document_id, grade = line.split()
        grade = int(grade)
        if unused != "Q0" or grade not in (0, 1):
            raise ValueError(f"unexpected human judgement: {line}")
        if document_id in judgements[query_id][1 - grade]:
            raise ValueError(f"conflicting human judgement: {line}")
        judgements[query_id][grade].add(document_id)
    selected = [
        query_id for query_id in topics
        if judgements[query_id][1] and len(judgements[query_id][0]) >= NEGATIVES_PER_QUERY
    ][:QUERY_COUNT]
    if len(selected) != QUERY_COUNT:
        raise ValueError(f"{language} has insufficient explicitly judged dev queries")

    needed = set().union(*(judgements[query_id][0] | judgements[query_id][1] for query_id in selected))
    documents = {}
    with gzip.open(root / f"{language}-corpus.jsonl.gz", "rt") as stream:
        for line in stream:
            row = json.loads(line)
            if row["docid"] in needed:
                if row["docid"] in documents:
                    raise ValueError(f"duplicate corpus passage: {row['docid']}")
                documents[row["docid"]] = f'{row["title"]}\n{row["text"]}'
    if needed - documents.keys():
        raise ValueError(f"{len(needed - documents.keys())} judged passages absent from corpus")

    cases = []
    selected_documents = set()
    for query_id in selected:
        positive = sorted(judgements[query_id][1])[0]
        query_words = words(topics[query_id])
        negatives = sorted(
            judgements[query_id][0],
            key=lambda docid: (-len(query_words & words(documents[docid])), docid),
        )[:NEGATIVES_PER_QUERY]
        selected_documents.update([positive, *negatives])
        cases.append({
            "id": f"{language}-{query_id}",
            "query": topics[query_id],
            "relevantIds": [positive],
            "negativeIds": negatives,
            "eligibleIds": [positive, *negatives],
        })
    return {
        "labelPolicy": "MIRACL dev: all eligible pairs explicitly judged 1/0 by native speakers; one positive and three lexically closest judged negatives per query",
        "source": {
            "dataset": "miracl/miracl",
            "judgementsRevision": "5be20db9509754dadad47689368639fcec739c00",
            "corpusRevision": "d921ec7e349ce0d28daf30b2da9da5ee698bef0d",
        },
        "documents": [{"id": docid, "text": documents[docid]} for docid in sorted(selected_documents)],
        "cases": cases,
    }


def main():
    root = Path(sys.argv[1]) if len(sys.argv) == 2 else Path("target/gemma-quality-data/miracl")
    for name, expected in SOURCE_SHA256.items():
        if digest(root / name) != expected:
            raise ValueError(f"pinned MIRACL snapshot differs: {name}")
    for language in ("sw", "bn"):
        path = root / f"{language}-judged-fixture.json"
        path.write_text(json.dumps(build(root, language), ensure_ascii=False, indent=2) + "\n")
        fixture = json.loads(path.read_text())
        print(language, len(fixture["cases"]), len(fixture["documents"]), digest(path))


if __name__ == "__main__":
    main()
