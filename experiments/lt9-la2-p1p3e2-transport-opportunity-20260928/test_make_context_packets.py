import unittest

from make_context_packets import context_windows, endpoint_forms, mask_term


class ContextPacketTests(unittest.TestCase):
    def test_context_windows_keep_focal_occurrence_and_bound_width(self):
        text = "before one two source three four after"
        windows = context_windows(text, "source", radius=2)
        self.assertEqual(windows, ["one two source three four"])

    def test_context_windows_cap_repeated_occurrences(self):
        text = "source a source b source c source"
        self.assertEqual(len(context_windows(text, "source", radius=1)), 3)

    def test_candidate_mask_uses_token_boundaries(self):
        self.assertEqual(mask_term("car carpool CAR", "car", "[SOURCE]"), "[SOURCE] carpool [SOURCE]")

    def test_direction_preserves_orientation_when_pair_is_sorted(self):
        candidate = {
            "candidate_id": "loan_to_debt:loan->debt",
            "direction": "loan->debt",
            "lexical_pair": ["debt", "loan"],
        }
        self.assertEqual(endpoint_forms(candidate), ("loan", "debt"))


if __name__ == "__main__":
    unittest.main()
