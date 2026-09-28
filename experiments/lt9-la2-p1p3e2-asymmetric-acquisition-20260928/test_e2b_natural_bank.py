import unittest
from collections import defaultdict

from build_e2b_natural_bank import (
    LANES,
    Occurrence,
    canonical_identity,
    content_tokens,
    digest_key,
    masked_context,
    split_candidates,
)


class AcquisitionHelpersTest(unittest.TestCase):
    def test_candidate_forms_are_masked_and_not_scoring_tokens(self):
        result = masked_context("The bank lent money near the river bank.", "bank", "bank", "water", 18)
        self.assertIsNotNone(result)
        context, visible = result
        self.assertIn("[SOURCE]", context)
        self.assertNotIn("bank", context.casefold())
        self.assertEqual(visible, len(content_tokens(context)) + 2)  # function words remain visible counts
        self.assertNotIn("source", content_tokens(context))

    def test_seeded_order_is_reproducible_and_namespaced(self):
        self.assertEqual(digest_key(7, "PAIR", "fiqa", "car->vehicle", "q1", "d1"),
                         digest_key(7, "PAIR", "fiqa", "car->vehicle", "q1", "d1"))
        self.assertNotEqual(digest_key(7, "QUERY", "q1"), digest_key(7, "DOC", "q1"))

    def test_split_assignment_keeps_dataset_scoped_ids_disjoint(self):
        datasets = ["fiqa", "scifact"]
        queries = defaultdict(list)
        docs = defaultdict(list)
        relation = "bank->lender"
        for dataset in datasets:
            for i in range(32):
                queries[(dataset, relation)].append(Occurrence(
                    dataset, f"q{i}", f"question [SOURCE] topicword{i} common evidence",
                    frozenset({f"topicword{i}", "common", "evidence"}), 4, "query"))
                docs[(dataset, relation)].append(Occurrence(
                    dataset, f"d{i}", f"document [TARGET] topicword{i} common evidence",
                    frozenset({f"topicword{i}", "common", "evidence"}), 4, "title+text"))
        lock = {"seed": 2026092801, "source_cohort": [{"name": name} for name in datasets]}
        rows, primary, report = split_candidates(lock, queries, docs)
        by_split = defaultdict(set)
        for row in primary:
            by_split[row["split"]].add(canonical_identity(row["dataset"], "query", row["query_id"]))
            by_split[row["split"]].add(canonical_identity(row["dataset"], "document", row["document_id"]))
        split_names = ["TRAIN-NEW", "DEV-NEW", "TEST-NEW"]
        for i, left in enumerate(split_names):
            for right in split_names[i + 1:]:
                self.assertFalse(by_split[left] & by_split[right])
        self.assertEqual(len([r for r in rows if r["population"] == "PRIMARY"]), 24)
        self.assertEqual(len([r for r in rows if r["population"] == "RESERVE"]), 12)
        self.assertFalse(any(r["label"] is not None for r in rows))
        self.assertTrue(all(r["lane"] in LANES for r in rows))
        self.assertEqual(report["primary_count"], 24)


if __name__ == "__main__":
    unittest.main()
