from __future__ import annotations

import unittest
from types import SimpleNamespace

from build_e2c_extension import allocate_relation, assign_packet_ids, digest


def candidate(dataset: str, query_id: str, document_id: str, label: str):
    return SimpleNamespace(
        relation="engine->motor", source="engine", target="motor", dataset=dataset,
        query_id=query_id, document_id=document_id, query_context=f"q {query_id}",
        document_context=f"d {document_id}", query_visible=4, document_visible=5,
        jaccard=0.25, shared=1, query_field="query", document_field="title+text",
        base_group_id=f"bg-{label}", pair_identity=f"{dataset}\x1f{query_id}\x1f{document_id}",
    )


class E2CExtensionBuilderTests(unittest.TestCase):
    def test_namespaced_keys_are_deterministic_and_separate(self):
        self.assertEqual(digest(2026092801, "PAIR", "fiqa", "r", "q", "d"),
                         digest(2026092801, "PAIR", "fiqa", "r", "q", "d"))
        self.assertNotEqual(digest(2026092801, "PAIR", "fiqa", "r", "q", "d"),
                            digest(2026092802, "PAIR", "fiqa", "r", "q", "d"))

    def test_allocator_keeps_query_document_identities_partition_disjoint(self):
        lock = {"extension": {
            "ordering": {"seed": 2026092801},
            "acquisition_lanes": {"order": ["SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY"],
                                  "dev_counts_per_relation": [1, 0, 0],
                                  "test_counts_per_relation": [1, 0, 0]},
            "identity_firewall": {"max_per_corpus_per_relation_dev": 3,
                                  "max_per_corpus_per_relation_test": 3},
        }}
        rows, report = allocate_relation(
            [candidate("fiqa", "q1", "d1", "a"), candidate("fiqa", "q2", "d2", "b")],
            "engine->motor", lock, {},
        )
        self.assertEqual(len(rows), 2)
        self.assertFalse(report["underfill"])
        self.assertEqual({row["split"] for row in rows}, {"DEV-EXT", "TEST-EXT"})
        self.assertTrue(all(row["label"] is None for row in rows))

    def test_allocator_fails_closed_on_corpus_cap(self):
        lock = {"extension": {
            "ordering": {"seed": 2026092801},
            "acquisition_lanes": {"order": ["SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY"],
                                  "dev_counts_per_relation": [1, 1, 0],
                                  "test_counts_per_relation": [0, 0, 0]},
            "identity_firewall": {"max_per_corpus_per_relation_dev": 1,
                                  "max_per_corpus_per_relation_test": 1},
        }}
        rows, report = allocate_relation(
            [candidate("fiqa", "q1", "d1", "a"), candidate("fiqa", "q2", "d2", "b")],
            "engine->motor", lock, {},
        )
        self.assertEqual(len(rows), 1)
        self.assertIn("engine->motor|SENSE_CONTRAST|DEV-EXT", report["underfill"])

    def test_packet_ids_are_stable(self):
        row = {"base_group_id": "bg-example"}
        a = assign_packet_ids([dict(row)], 2026092801)[0]["packet_id"]
        b = assign_packet_ids([dict(row)], 2026092801)[0]["packet_id"]
        self.assertEqual(a, b)
        self.assertTrue(a.startswith("e2c-"))


if __name__ == "__main__":
    unittest.main()
