import unittest

from direct_endpoint_lfm import category, prompt


class DirectEndpointTests(unittest.TestCase):
    def test_direction_is_explicit_and_context_terms_are_masked(self):
        text = prompt("bank", "shore", "The [FOCAL] bank met the shore")
        self.assertIn("RELATION: bank -> shore", text)
        self.assertIn("[FOCAL]", text)
        self.assertNotIn("bank met", text)
        self.assertNotIn("the shore", text)

    def test_weak_negative_keeps_its_provenance(self):
        self.assertEqual(category({"label": "DIFFERENT",
                                   "evidence_type": "COINCO_FALSE_IMPLICIT_WEAK_NEGATIVE"}),
                         "WEAK_DIFFERENT")


if __name__ == "__main__":
    unittest.main()
