import importlib.util
import unittest
from pathlib import Path

import numpy as np


spec = importlib.util.spec_from_file_location(
    "comparison", Path(__file__).with_name("compare-embedding-models.py")
)
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


class RetrievalMetricsTest(unittest.TestCase):
    def test_ranking_and_negative_control(self):
        rows = [
            {"chunkId": "a", "sourceIds": ["source-a"]},
            {"chunkId": "b", "sourceIds": ["source-b"]},
        ]
        cases = [
            {
                "id": "query",
                "relevantSourceIds": ["source-b"],
                "relevantChunkIds": ["b"],
                "expectedNoEvidence": False,
            },
            {"id": "negative", "expectedNoEvidence": True},
        ]
        scores = comparison.metrics(
            cases,
            rows,
            np.array([[1.0, 0], [0, 1.0]]),
            np.array([[1.0, 0], [1.0, 0]]),
            2,
        )
        self.assertEqual(scores["evaluatedPositiveCases"], 1)
        self.assertEqual(scores["excludedNegativeCases"], 1)
        self.assertEqual(scores["fileRecallAt10"], 1)
        self.assertEqual(scores["chunkRecallAt10"], 1)
        self.assertEqual(scores["mrr"], 0.5)

    def test_truncation_renormalizes_before_ranking(self):
        rows = [
            {"chunkId": "a", "sourceIds": ["a"]},
            {"chunkId": "b", "sourceIds": ["b"]},
        ]
        cases = [{
            "id": "query",
            "relevantSourceIds": ["b"],
            "relevantChunkIds": ["b"],
            "expectedNoEvidence": False,
        }]
        scores = comparison.metrics(
            cases,
            rows,
            np.array([[0.2, 0.4, 1.0], [0.9, 0.1, 0.0]]),
            np.array([[1.0, 0.0, 0.0]]),
            2,
        )
        self.assertEqual(scores["mrr"], 1)
        self.assertEqual(scores["cases"][0]["topChunkIds"][0], "b")


if __name__ == "__main__":
    unittest.main()
