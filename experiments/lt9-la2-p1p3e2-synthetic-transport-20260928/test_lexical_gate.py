import unittest

from lexical_gate import context, decide, features


class LexicalGateTests(unittest.TestCase):
    def test_candidate_forms_do_not_vote(self):
        left = context("[FOCAL] bank lender river", "bank", "lender")
        right = context("[FOCAL] river", "bank", "lender")
        self.assertEqual(left.content, frozenset({"river"}))
        self.assertEqual(right.content, frozenset({"river"}))
        self.assertGreater(features(left, right, {})[4], 0)

    def test_absent_context_has_no_content_anchor(self):
        empty = context("[FOCAL]", "car", "vehicle")
        full = context("the [SOURCE] engine runs", "car", "vehicle")
        self.assertEqual(features(empty, full, {})[4], 0)

    def test_conservative_gate_abstains_without_anchor(self):
        model = {
            "mean": [0.0] * 18, "scale": [1.0] * 18,
            "sufficiency": {"weights": [0.0] * 18, "bias": 10.0},
            "compatibility": {"weights": [0.0] * 18, "bias": 10.0},
            "sufficiency_threshold": 0.5, "allow_threshold": 0.5,
            "refuse_threshold": 0.2, "require_content_anchor": True,
        }
        self.assertEqual(decide(model, (0.0,) * 18), "ABSTAIN")


if __name__ == "__main__":
    unittest.main()
