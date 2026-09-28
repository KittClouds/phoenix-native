from __future__ import annotations

import unittest

from open_e2b_reserves import make_packets


RELATIONS = [
    "bank->lender", "bank->water", "car->vehicle", "credit->loan", "engine->motor",
    "insurance->coverage", "loan->debt", "stock->bond", "vehicle->car",
]


def fixture():
    unsupported = [relation for relation in RELATIONS if relation != "stock->bond"]
    gate = {
        "status": "SUPPORT_GATE_COMPLETE_NO_FEATURE_JOIN_NO_FIT",
        "relations": {relation: {} for relation in RELATIONS},
        "gate": {
            "next_action": "OPEN_FROZEN_RESERVES_FOR_UNSUPPORTED_RELATIONS",
            "relations_unsupported": unsupported,
        },
    }
    ledger = []
    for relation_i, relation in enumerate(RELATIONS):
        for candidate_i in range(24):
            split = "DEV-NEW" if candidate_i < 5 else "TEST-NEW" if candidate_i < 10 else "TRAIN-NEW"
            ledger.append({
                "packet_id": f"p-{relation_i}-{candidate_i}",
                "base_group_id": f"pg-{relation_i}-{candidate_i}",
                "relation": relation,
                "population": "PRIMARY",
                "split": split,
                "lane": "SEMANTIC_NEAR",
                "dataset": "fixture",
                "query_id": f"q-{relation_i}-{candidate_i}",
                "document_id": f"d-{relation_i}-{candidate_i}",
            })
        for reserve_i in range(12):
            ledger.append({
                "packet_id": f"r-{relation_i}-{reserve_i}",
                "base_group_id": f"rg-{relation_i}-{reserve_i}",
                "relation": relation,
                "population": "RESERVE",
                "split": "TRAIN-NEW",
                "lane": ("SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY")[reserve_i % 3],
                "dataset": "fixture",
                "query_id": f"rq-{relation_i}-{reserve_i}",
                "document_id": f"rd-{relation_i}-{reserve_i}",
                "query_context": "context around [SOURCE] with ordinary nearby words",
                "document_context": "context around [TARGET] with different nearby words",
            })
    return gate, ledger, {"seed": 2026092801}


class ReserveOpeningTests(unittest.TestCase):
    def test_opens_only_unsupported_relation_queues(self):
        gate, ledger, lock = fixture()
        packets, receipt = make_packets(gate, ledger, lock)
        self.assertEqual(len(packets), 8 * 12)
        self.assertEqual(set(receipt["opened_relations"]), {r for r in RELATIONS if r != "stock->bond"})
        self.assertNotIn("stock->bond", {row["lexical_relation"] for row in packets})
        self.assertTrue(all(set(row) == {"packet_id", "lexical_relation", "query_context", "document_context"} for row in packets))

    def test_refuses_dev_test_identity_overlap(self):
        gate, ledger, lock = fixture()
        reserve = next(row for row in ledger if row["population"] == "RESERVE")
        reserve["query_id"] = next(
            row["query_id"] for row in ledger
            if row["population"] == "PRIMARY" and row["split"] == "DEV-NEW"
        )
        with self.assertRaises(ValueError):
            make_packets(gate, ledger, lock)

    def test_requires_support_gate_authorization(self):
        gate, ledger, lock = fixture()
        gate["gate"]["next_action"] = "PROCEED_TO_NEXT_PREREGISTERED_GATE"
        with self.assertRaises(ValueError):
            make_packets(gate, ledger, lock)


if __name__ == "__main__":
    unittest.main()
