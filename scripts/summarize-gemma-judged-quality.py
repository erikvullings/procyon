"""Summarize matched human-judged candidate pools without hiding case failures."""

import hashlib
import json
import random
import sys
from pathlib import Path

DIMENSIONS = ("128", "256", "512", "768")
RESAMPLES = 10_000


def read(path):
    return json.loads(path.read_text())


def summarize(fixture_path, e5_path, gemma_path):
    fixture_bytes = fixture_path.read_bytes()
    fixture = json.loads(fixture_bytes)
    expected = hashlib.sha256(fixture_bytes).hexdigest()
    e5 = read(e5_path)
    gemma = read(gemma_path)
    if (
        e5.get("model") != "intfloat/multilingual-e5-small"
        or e5.get("revision") != "614241f622f53c4eeff9890bdc4f31cfecc418b3"
        or e5.get("dimensions") != 384
        or gemma.get("model") != "google/embeddinggemma-2"
        or gemma.get("revision") != "914f7f89142e33e77833254d9c9b90c3cef7303b"
        or gemma.get("measurementWidth") != 768
    ):
        raise ValueError("model identity or dimension differs from the signed candidate")
    if expected != e5["corpusSha256"] or expected != gemma["result"]["corpusSha256"]:
        raise ValueError("the models did not rank the same pinned corpus/query split")
    cases = fixture["cases"]
    expected_ids = [case["id"] for case in cases]
    e5_cases = e5["result"]["cases"]
    if [case["id"] for case in e5_cases] != expected_ids:
        raise ValueError("E5 case order or count differs from the fixture")
    if not all(
        case["relevantIds"]
        and set(case["relevantIds"]) <= set(case["eligibleIds"])
        and len(case["eligibleIds"]) > len(case["relevantIds"])
        and (
            "negativeIds" not in case
            or set(case["negativeIds"]) == set(case["eligibleIds"]) - set(case["relevantIds"])
        )
        for case in cases
    ):
        raise ValueError("fixture lacks judged positive or candidate negatives")
    gemma_cases = {}
    for dimension in DIMENSIONS:
        measured = gemma["result"]["dimensions"][dimension]["cases"]
        if [case["id"] for case in measured] != expected_ids:
            raise ValueError(f"Gemma {dimension}d case order or count differs")
        gemma_cases[dimension] = measured

    detailed_failures = []
    for index, case in enumerate(cases):
        e5_case = e5_cases[index]
        native = {dimension: gemma_cases[dimension][index] for dimension in DIMENSIONS}
        if e5_case["firstRelevantRank"] != 1 or any(
            result["firstRelevantRank"] != 1 for result in native.values()
        ):
            detailed_failures.append({
                "id": case["id"],
                "e5Rank": e5_case["firstRelevantRank"],
                "e5TopIds": e5_case["topIds"],
                "gemmaRanks": {dimension: result["firstRelevantRank"] for dimension, result in native.items()},
                "gemmaTopIds": {dimension: result["topIds"] for dimension, result in native.items()},
            })

    e5_hits = [int(case["firstRelevantRank"] == 1) for case in e5_cases]
    n = len(e5_hits)
    dimensions = {}
    for dimension in DIMENSIONS:
        gemma_hits = [int(case["firstRelevantRank"] == 1) for case in gemma_cases[dimension]]
        paired = [gemma_hit - e5_hit for e5_hit, gemma_hit in zip(e5_hits, gemma_hits, strict=True)]
        generator = random.Random(19_870 + int(dimension))
        samples = sorted(
            sum(paired[generator.randrange(n)] for _ in range(n)) / n
            for _ in range(RESAMPLES)
        )
        dimensions[dimension] = {
            "gemmaHitAt1": sum(gemma_hits) / n,
            "gemmaMrr": gemma["result"]["dimensions"][dimension]["mrr"],
            "pairedGemmaMinusE5HitAt1": sum(paired) / n,
            "pairedBootstrap95PercentileInterval": [
                samples[int(0.025 * RESAMPLES)],
                samples[int(0.975 * RESAMPLES) - 1],
            ],
            "gemmaWinsTiesLosses": [
                paired.count(1), paired.count(0), paired.count(-1)
            ],
        }
    return {
        "fixtureSha256": expected,
        "queries": n,
        "uniqueDocuments": len(fixture["documents"]),
        "judgedPositiveCandidates": sum(len(case["relevantIds"]) for case in cases),
        "judgedNegativeCandidates": sum(
            len(case["eligibleIds"]) - len(case["relevantIds"]) for case in cases
        ),
        "e5HitAt1": e5["result"]["hitAt1"],
        "e5Mrr": e5["result"]["mrr"],
        "dimensions": dimensions,
        "nonPerfectCases": detailed_failures,
    }


def main():
    if len(sys.argv) != 3:
        raise ValueError("usage: summarize-gemma-judged-quality.py MIRACL_DIR CODE_DIR")
    multilingual, code = map(Path, sys.argv[1:])
    data = {
        "method": "exact cosine, one positive and three human-judged pooled negatives per MIRACL dev query; code uses unanimous grades >=2 vs 0",
        "uncertainty": "10,000 seeded paired-query bootstrap resamples of hit@1 difference, conditional on these deterministic small candidate pools; no population-wide or cross-platform confidence claim",
        "swahili": summarize(
            multilingual / "sw-judged-fixture.json",
            multilingual / "e5-sw.json",
            multilingual / "gemma-sw.json",
        ),
        "bengali": summarize(
            multilingual / "bn-judged-fixture.json",
            multilingual / "e5-bn.json",
            multilingual / "gemma-bn.json",
        ),
        "code": summarize(
            code / "code-expanded-fixture.json",
            code / "e5-code-expanded.json",
            code / "gemma-code-expanded.json",
        ),
    }
    print(json.dumps(data, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
