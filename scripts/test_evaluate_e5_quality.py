import unittest

import numpy as np

from evaluate_e5_quality import rank_cases


class RankedCaseTests(unittest.TestCase):
    def setUp(self):
        self.corpus = {
            "documents": [{"id": "positive"}, {"id": "unjudged"}, {"id": "negative"}],
            "cases": [{
                "id": "judged",
                "relevantIds": ["positive"],
                "eligibleIds": ["positive", "negative"],
            }],
        }
        self.vectors = [
            np.array([0.8, 0.6]),
            np.array([1.0, 0.0]),
            np.array([0.0, 1.0]),
        ]
        self.query = [np.array([1.0, 0.0])]

    def test_unjudged_candidate_cannot_displace_assessed_positive(self):
        result = rank_cases(self.corpus, self.vectors, self.query)
        self.assertEqual(result["hitAt1"], 1.0)
        self.assertEqual(result["cases"][0]["topIds"], ["positive", "negative"])

    def test_rejects_unjudged_positive(self):
        self.corpus["cases"][0]["relevantIds"] = ["unjudged"]
        with self.assertRaisesRegex(ValueError, "unjudged relevant"):
            rank_cases(self.corpus, self.vectors, self.query)


if __name__ == "__main__":
    unittest.main()
