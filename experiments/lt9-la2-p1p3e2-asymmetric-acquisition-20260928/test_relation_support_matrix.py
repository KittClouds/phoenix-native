"""Small invariant tests for the P1P3E2 relation support audit."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("build_relation_support_matrix.py")
SPEC = importlib.util.spec_from_file_location("support_matrix", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
support_matrix = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(support_matrix)


class RelationSupportMatrixTests(unittest.TestCase):
    def test_sparse_bins_match_frozen_boundaries(self) -> None:
        self.assertEqual(support_matrix.sparse_bin(0), "0-2")
        self.assertEqual(support_matrix.sparse_bin(2), "0-2")
        self.assertEqual(support_matrix.sparse_bin(3), "3-5")
        self.assertEqual(support_matrix.sparse_bin(5), "3-5")
        self.assertEqual(support_matrix.sparse_bin(6), "6+")

    def test_focal_placeholders_do_not_inflate_context_support(self) -> None:
        self.assertEqual(support_matrix.side_count(["[SOURCE] pays [TARGET] in cash"]), 3)
        self.assertEqual(support_matrix.side_count(["[SOURCE] [TARGET]"]), 0)

    def test_context_identity_is_deterministic_and_relation_bound(self) -> None:
        base = {
            "lexical_relation": ["car", "vehicle"],
            "orientation": "query_source_to_document_counterpart",
            "query_contexts": ["[SOURCE] drives on roads"],
            "document_contexts": ["[TARGET] is used on roads"],
        }
        self.assertEqual(support_matrix.context_identity(base), support_matrix.context_identity(dict(base)))
        changed = {**base, "lexical_relation": ["vehicle", "car"]}
        self.assertNotEqual(support_matrix.context_identity(base), support_matrix.context_identity(changed))

    def test_discordant_duplicate_is_excluded_not_relabelled_unknown(self) -> None:
        row = {
            "relation": "car->vehicle",
            "context_identity_sha256": "same-payload",
            "source_key": "source-a",
            "label": "SAME",
        }
        other = {**row, "source_key": "source-b", "label": "DIFFERENT"}
        result = support_matrix.deduplicated_support([row, other])
        self.assertEqual(result["unique_natural_context_pairs"], 1)
        self.assertEqual(result["unique_pairs_with_cross_pass_or_within_pass_label_conflict"], 1)
        self.assertNotIn("car->vehicle", result["by_relation"])
        self.assertEqual(result["conflicts"][0]["disposition"], "EXCLUDED_FROM_POOLED_DEFINITE_CLASS_COUNTS_NOT_RELABELLED_UNKNOWN")

    def test_identical_duplicates_count_once_in_pooled_support(self) -> None:
        row = {
            "relation": "car->vehicle",
            "context_identity_sha256": "same-payload",
            "source_key": "source-a",
            "label": "SAME",
        }
        other = {**row, "source_key": "source-b"}
        result = support_matrix.deduplicated_support([row, other])
        self.assertEqual(result["unique_natural_context_pairs"], 1)
        self.assertEqual(result["by_relation"]["car->vehicle"]["labels"]["SAME"], 1)
        self.assertEqual(result["unique_pairs_with_cross_pass_or_within_pass_label_conflict"], 0)


if __name__ == "__main__":
    unittest.main()
