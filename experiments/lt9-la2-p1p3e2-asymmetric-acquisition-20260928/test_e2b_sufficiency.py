from __future__ import annotations

import unittest

from assess_e2b_sufficiency import assess


def fixture_data():
    relations = [
        ("bank", "lender"), ("bank", "water"), ("car", "vehicle"),
        ("credit", "loan"), ("engine", "motor"), ("insurance", "coverage"),
        ("loan", "debt"), ("stock", "bond"), ("vehicle", "car"),
    ]
    lane_split = {
        "SEMANTIC_NEAR": ["TRAIN-NEW"] * 5 + ["DEV-NEW"] + ["TEST-NEW"] * 2,
        "SENSE_CONTRAST": ["TRAIN-NEW"] * 5 + ["DEV-NEW"] * 2 + ["TEST-NEW"],
        "SPARSE_OR_BOUNDARY": ["TRAIN-NEW"] * 4 + ["DEV-NEW"] * 2 + ["TEST-NEW"] * 2,
    }
    ledger = []
    labels = []
    for relation_index, (source, target) in enumerate(relations):
        relation = f"{source}->{target}"
        train_i = 0
        for lane in lane_split:
            for local_i, split in enumerate(lane_split[lane]):
                packet_id = f"p-{relation_index}-{lane}-{local_i}"
                label = "SAME" if train_i < 4 else "DIFFERENT" if train_i < 8 else "UNKNOWN"
                if split != "TRAIN-NEW":
                    label = ("SAME", "DIFFERENT", "UNKNOWN")[(local_i + relation_index) % 3]
                else:
                    train_i += 1
                ledger.append({
                    "packet_id": packet_id,
                    "base_group_id": f"g-{packet_id}",
                    "relation": relation,
                    "population": "PRIMARY",
                    "split": split,
                    "lane": lane,
                    "dataset": "fixture",
                })
                labels.append({"packet_id": packet_id, "judgment": label})
        for reserve_i in range(12):
            packet_id = f"r-{relation_index}-{reserve_i}"
            ledger.append({
                "packet_id": packet_id,
                "base_group_id": f"g-{packet_id}",
                "relation": relation,
                "population": "RESERVE",
                "split": "TRAIN-NEW",
                "lane": ("SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY")[reserve_i % 3],
                "dataset": "fixture",
            })
    lock = {
        "support_contract": {
            "natural_train_same_min": 8,
            "natural_train_different_min": 8,
            "train_new_same_min": 4,
            "train_new_different_min": 4,
            "legacy_may_contribute": True,
        },
        "natural_unknown_monitor": {"minimum_count": 20, "minimum_relations": 4},
        "relations": [
            {"source": source, "target": target, "legacy_same": 4, "legacy_different": 4}
            for source, target in relations
        ],
    }
    return labels, ledger, lock


class SufficiencyTests(unittest.TestCase):
    def test_support_uses_train_new_and_legacy_only(self):
        labels, ledger, lock = fixture_data()
        result = assess(labels, ledger, lock)
        for data in result["relations"].values():
            self.assertEqual(data["support_state"], "SUPPORTED")
            self.assertEqual(data["combined_train_support"]["SAME"], 8)
            self.assertEqual(data["combined_train_support"]["DIFFERENT"], 8)

    def test_dev_and_test_do_not_contribute_to_support(self):
        labels, ledger, lock = fixture_data()
        relation = "bank->lender"
        labels_by_id = {row["packet_id"]: row for row in labels}
        for row in ledger:
            if row["relation"] == relation and row["population"] == "PRIMARY" and row["split"] == "TRAIN-NEW":
                labels_by_id[row["packet_id"]]["judgment"] = "UNKNOWN"
        result = assess(labels, ledger, lock)
        self.assertEqual(result["relations"][relation]["support_state"], "UNSUPPORTED_ABSTAIN")

    def test_semantic_near_and_sparse_unknown_counts_are_separate(self):
        labels, ledger, lock = fixture_data()
        result = assess(labels, ledger, lock)
        lane_counts = result["relations"]["bank->lender"]["lane_counts_by_split"]["TRAIN-NEW"]
        self.assertGreaterEqual(lane_counts["SEMANTIC_NEAR"]["SAME"], 1)
        self.assertGreaterEqual(lane_counts["SPARSE_OR_BOUNDARY"]["UNKNOWN"], 1)

    def test_rejects_missing_or_invalid_label_rows(self):
        labels, ledger, lock = fixture_data()
        with self.assertRaises(ValueError):
            assess(labels[:-1], ledger, lock)
        labels, ledger, lock = fixture_data()
        labels[0]["judgment"] = "MAYBE"
        with self.assertRaises(ValueError):
            assess(labels, ledger, lock)


if __name__ == "__main__":
    unittest.main()
