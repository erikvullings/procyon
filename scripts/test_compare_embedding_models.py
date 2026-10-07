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
    def test_prompt_routing_for_code_and_documents(self):
        class Recorder:
            def __init__(self):
                self.calls = []

            def encode(self, texts, **kwargs):
                self.calls.append((texts, kwargs))
                return np.array([[1., 0.]])

        gemma = Recorder()
        comparison.encode(gemma, "gemma", ["find function"], True, "code")
        comparison.encode(gemma, "gemma", ["fn hello() {}"], False, "code")
        self.assertEqual(gemma.calls[0][1]["prompt_name"], "CodeRetrieval")
        self.assertEqual(gemma.calls[1][1]["prompt_name"], "Document")
        comparison.encode(gemma, "gemma", ["find manual"], True, "knowledge")
        self.assertEqual(gemma.calls[2][1]["prompt_name"], "SearchQuery")
        e5 = Recorder()
        comparison.encode(e5, "e5", ["Find Code"], True, "code")
        self.assertEqual(e5.calls[0][0], ["query: find code"])

    def test_code_fixture_and_hard_negatives(self):
        import json
        corpus = json.loads(comparison.CODE_CORPUS.read_text())
        ids = {document["id"] for document in corpus["documents"]}
        self.assertEqual(len(ids), 12)
        self.assertEqual(len(corpus["cases"]), 6)
        self.assertTrue(all(set(case["relevantIds"]) <= ids for case in corpus["cases"]))
        self.assertTrue(all(len(case["relevantIds"]) == 1 for case in corpus["cases"]))

    def test_code_metrics_ranks_relevant_at_first_position(self):
        rows = [{"id": "positive"}, {"id": "distractor"}]
        cases = [{"id": "find", "relevantIds": ["positive"]}]
        result = comparison.code_metrics(cases, rows,
                                         np.array([[1., 0.], [0., 1.]]),
                                         np.array([[1., 0.]]), 2)
        self.assertEqual(result["recallAt1"], 1)
        self.assertEqual(result["mrr"], 1)

    def test_image_retrieval_excludes_query_itself(self):
        images = [("red-circle-original", None), ("red-circle-jpeg", None),
                  ("red-square", None)]
        vectors = np.array([[1., 0.], [0.9, 0.1], [0., 1.]])
        result = comparison.image_metrics(images, vectors,
                                          [("red-circle", vectors[0], True)])
        self.assertEqual(result["recallAt2"], 1)
        self.assertEqual(result["cases"][0]["topIds"][0], "red-circle-jpeg")

    def test_generated_image_fixture_has_distinct_variants_and_negatives(self):
        images = comparison.image_fixture()
        self.assertEqual(len(images), 12)
        self.assertEqual(len({key for key, _ in images}), 12)
        self.assertTrue(all(image.size[0] > 0 for _, image in images))

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
