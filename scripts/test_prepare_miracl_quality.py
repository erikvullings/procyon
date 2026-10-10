import gzip
import json
import runpy
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

build = runpy.run_path(str(Path(__file__).with_name("prepare-miracl-quality-fixtures.py")))["build"]


class JudgedFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "sw-topics.dev.tsv").write_text("q\tbuffered file reader\n")
        (self.root / "sw-qrels.dev.tsv").write_text(
            "q Q0 positive 1\n"
            "q Q0 near 0\n"
            "q Q0 distant 0\n"
            "q Q0 medium 0\n"
            "q Q0 fourth 0\n"
        )
        documents = [
            {"docid": "positive", "title": "A", "text": "buffered reader"},
            {"docid": "near", "title": "B", "text": "buffered file reader"},
            {"docid": "medium", "title": "C", "text": "file reader"},
            {"docid": "distant", "title": "D", "text": "other text"},
            {"docid": "fourth", "title": "E", "text": "buffered"},
            {"docid": "unjudged", "title": "F", "text": "buffered file reader"},
        ]
        with gzip.open(self.root / "sw-corpus.jsonl.gz", "wt") as stream:
            for document in documents:
                stream.write(json.dumps(document) + "\n")

    def test_only_explicitly_judged_negatives_enter_the_ranked_pool(self):
        with patch.dict(build.__globals__, {"QUERY_COUNT": 1}):
            fixture = build(self.root, "sw")
        case = fixture["cases"][0]
        self.assertEqual(case["relevantIds"], ["positive"])
        self.assertEqual(case["negativeIds"], ["near", "medium", "fourth"])
        self.assertNotIn("unjudged", case["eligibleIds"])
        self.assertEqual(len(fixture["documents"]), 4)

    def test_rejects_conflicting_or_missing_judgements(self):
        with (self.root / "sw-qrels.dev.tsv").open("a") as stream:
            stream.write("q Q0 positive 0\n")
        with patch.dict(build.__globals__, {"QUERY_COUNT": 1}):
            with self.assertRaisesRegex(ValueError, "conflicting human judgement"):
                build(self.root, "sw")
        (self.root / "sw-qrels.dev.tsv").write_text("q Q0 positive 1\nq Q0 near 0\n")
        with patch.dict(build.__globals__, {"QUERY_COUNT": 1}):
            with self.assertRaisesRegex(ValueError, "insufficient explicitly judged"):
                build(self.root, "sw")


if __name__ == "__main__":
    unittest.main()
