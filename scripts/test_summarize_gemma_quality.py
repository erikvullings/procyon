import hashlib
import json
import runpy
import tempfile
import unittest
from pathlib import Path

summarize = runpy.run_path(
    str(Path(__file__).with_name("summarize-gemma-judged-quality.py"))
)["summarize"]


class MatchedSummaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fixture = self.root / "fixture.json"
        self.e5 = self.root / "e5.json"
        self.gemma = self.root / "gemma.json"
        self.fixture.write_text(json.dumps({
            "documents": [{"id": "positive"}, {"id": "negative"}],
            "cases": [
                {"id": "a", "relevantIds": ["positive"], "eligibleIds": ["positive", "negative"]},
                {"id": "b", "relevantIds": ["positive"], "eligibleIds": ["positive", "negative"]},
            ],
        }))
        digest = hashlib.sha256(self.fixture.read_bytes()).hexdigest()
        baseline = [
            {"id": "a", "firstRelevantRank": 1, "topIds": ["positive", "negative"]},
            {"id": "b", "firstRelevantRank": 2, "topIds": ["negative", "positive"]},
        ]
        native = [
            {"id": "a", "firstRelevantRank": 2, "topIds": ["negative", "positive"]},
            {"id": "b", "firstRelevantRank": 1, "topIds": ["positive", "negative"]},
        ]
        self.e5.write_text(json.dumps({
            "model": "intfloat/multilingual-e5-small",
            "revision": "614241f622f53c4eeff9890bdc4f31cfecc418b3",
            "dimensions": 384,
            "corpusSha256": digest,
            "result": {"hitAt1": 0.5, "mrr": 0.75, "cases": baseline},
        }))
        self.gemma.write_text(json.dumps({
            "model": "google/embeddinggemma-2",
            "revision": "914f7f89142e33e77833254d9c9b90c3cef7303b",
            "measurementWidth": 768,
            "result": {
                "corpusSha256": digest,
                "dimensions": {
                    str(width): {"mrr": 0.75, "cases": native}
                    for width in (128, 256, 512, 768)
                },
            },
        }))

    def test_reports_both_sides_of_paired_failures(self):
        result = summarize(self.fixture, self.e5, self.gemma)
        self.assertEqual(result["dimensions"]["128"]["gemmaWinsTiesLosses"], [1, 0, 1])
        self.assertEqual(result["dimensions"]["128"]["pairedGemmaMinusE5HitAt1"], 0)
        self.assertEqual([case["id"] for case in result["nonPerfectCases"]], ["a", "b"])

    def test_refuses_different_corpus_bytes(self):
        self.fixture.write_text(self.fixture.read_text() + "\n")
        with self.assertRaisesRegex(ValueError, "same pinned corpus"):
            summarize(self.fixture, self.e5, self.gemma)

    def test_refuses_wrong_model_identity(self):
        result = json.loads(self.e5.read_text())
        result["revision"] = "unverified"
        self.e5.write_text(json.dumps(result))
        with self.assertRaisesRegex(ValueError, "model identity"):
            summarize(self.fixture, self.e5, self.gemma)


if __name__ == "__main__":
    unittest.main()
