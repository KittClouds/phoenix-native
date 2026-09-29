import unittest

from score_qps_lanes import merge


class QpsLaneTests(unittest.TestCase):
    def test_refused_expansion_cannot_enter_baseline(self):
        row = {
            "baseline_hits": [{"ordinal": 1, "score": 2.0}],
            "expanded_hits": [{"ordinal": 2, "score": 3.0},
                              {"ordinal": 1, "score": 4.0}],
        }
        self.assertEqual([hit["ordinal"] for hit in merge(row, set())], [1])
        self.assertEqual([hit["ordinal"] for hit in merge(row, {2})], [2, 1])
        # Original hit keeps its original lexical score.
        self.assertEqual(merge(row, {1})[0]["score"], 2.0)


if __name__ == "__main__":
    unittest.main()
