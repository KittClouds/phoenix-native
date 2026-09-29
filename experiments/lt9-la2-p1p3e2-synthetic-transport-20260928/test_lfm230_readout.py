import unittest

from lfm230_readout import category, serialize


class LfmReadoutTests(unittest.TestCase):
    def test_candidate_strings_are_only_in_relation_header(self):
        prompt = serialize("bank", "shore", "The [SOURCE] touched the bank.",
                           "A shore beside the [TARGET].")
        self.assertIn("RELATION: bank -> shore", prompt)
        self.assertIn("[FOCAL]", prompt)
        self.assertNotIn("bank.", prompt)
        self.assertNotIn("shore beside", prompt)

    def test_explicit_rejection_not_folded_into_weak_negative(self):
        row = {"label": "DIFFERENT", "left_evidence": "SWORDS_FALSE_VOTE",
               "right_evidence": "COINCO_TRUE_IMPLICIT"}
        self.assertEqual(category(row), "STRONG_DIFFERENT")


if __name__ == "__main__":
    unittest.main()
