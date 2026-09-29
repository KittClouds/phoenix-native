import unittest
import importlib.util
from pathlib import Path
import sys


MODULE_PATH = Path(__file__).with_name("build_seed_bank.py")
SPEC = importlib.util.spec_from_file_location("build_seed_bank", MODULE_PATH)
assert SPEC and SPEC.loader
bank = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bank
SPEC.loader.exec_module(bank)


class SeedBankTests(unittest.TestCase):
    def test_masks_exact_target_span(self):
        self.assertEqual(bank.mask_target("A car moved", 2, "car"), "A [FOCAL] moved")

    def test_rejects_bad_target_offset(self):
        with self.assertRaises(ValueError):
            bank.mask_target("A car moved", 0, "car")

    def test_swords_votes_keep_explicit_boundary(self):
        label, source, confidence, counts = bank.source_label(
            "SWORDS_V1_1_DEV", ["TRUE", "TRUE", "FALSE"]
        )
        self.assertEqual((label, source), ("SAME", "SWORDS_TRUE_VOTE"))
        self.assertAlmostEqual(confidence, 2 / 3)
        self.assertEqual(counts["TRUE"], 2)

    def test_coinco_absent_candidate_is_marked_weak(self):
        label, source, confidence, _ = bank.source_label(
            "COINCO_DEV", ["FALSE_IMPLICIT"]
        )
        self.assertEqual(label, "DIFFERENT")
        self.assertEqual(source, "COINCO_FALSE_IMPLICIT_WEAK_NEGATIVE")
        self.assertEqual(confidence, 0.25)

    def test_pair_requires_both_positive_endpoints(self):
        from dataclasses import replace

        base = bank.Observation(
            observation_id="a",
            dataset="SWORDS_V1_1_DEV",
            context_id="source-context",
            context_occurrence_id="source-occurrence",
            context_group_id="source-group",
            context_text="The [FOCAL] works.",
            masked_context="The [FOCAL] works.",
            source_surface="car",
            source_lemma="car",
            target_surface="vehicle",
            target_lemma="vehicle",
            pos="NOUN",
            label="SAME",
            evidence_type="SWORDS_TRUE_VOTE",
            confidence=1.0,
            vote_counts={"TRUE": 3},
            split="train",
        )
        self.assertEqual(bank.pair_label(base, base)[0], "SAME")
        self.assertEqual(bank.pair_label(base, replace(base, label="DIFFERENT"))[0], "DIFFERENT")
        self.assertEqual(bank.pair_label(base, replace(base, label="UNKNOWN"))[0], "UNKNOWN")

    def test_split_is_repeatable(self):
        key = "a" * 64
        self.assertEqual(bank.stable_split(key), bank.stable_split(key))


if __name__ == "__main__":
    unittest.main()
