from __future__ import annotations

import unittest

from seal_e2b_reserve_judgments import seal


class ReserveJudgmentSealTests(unittest.TestCase):
    def setUp(self):
        self.packets = [{"packet_id": "p-1"}, {"packet_id": "p-2"}]
        self.template = [
            {"packet_id": "p-1", "judgment": None},
            {"packet_id": "p-2", "judgment": None},
        ]
        self.opening = {
            "status": "FROZEN_RESERVES_OPENED_FOR_TRAIN_SUPPORT_ONLY",
            "opened_candidate_count": 2,
            "opened_relations": ["a->b"],
            "boundaries": {"reserve_candidates_assigned_to_train_new_only": True},
        }

    def test_canonicalizes_valid_labels_and_preserves_order(self):
        submitted = [
            {"packet_id": "p-1", "judgment": "SAME"},
            {"packet_id": "p-2", "judgment": "UNKNOWN"},
        ]
        canonical, receipt = seal(self.packets, self.template, submitted, self.opening)
        self.assertEqual(canonical, submitted)
        self.assertEqual(receipt["label_counts"], {"SAME": 1, "DIFFERENT": 0, "UNKNOWN": 1})
        self.assertFalse(receipt["boundaries"]["private_ledger_read"])

    def test_rejects_reordered_or_missing_packet_ids(self):
        submitted = [
            {"packet_id": "p-2", "judgment": "SAME"},
            {"packet_id": "p-1", "judgment": "DIFFERENT"},
        ]
        with self.assertRaises(ValueError):
            seal(self.packets, self.template, submitted, self.opening)

    def test_rejects_invalid_labels(self):
        submitted = [
            {"packet_id": "p-1", "judgment": "YES"},
            {"packet_id": "p-2", "judgment": "DIFFERENT"},
        ]
        with self.assertRaises(ValueError):
            seal(self.packets, self.template, submitted, self.opening)

    def test_rejects_non_training_or_unsealed_opening(self):
        submitted = [
            {"packet_id": "p-1", "judgment": "SAME"},
            {"packet_id": "p-2", "judgment": "DIFFERENT"},
        ]
        wrong_opening = dict(self.opening)
        wrong_opening["status"] = "UNSEALED"
        with self.assertRaises(ValueError):
            seal(self.packets, self.template, submitted, wrong_opening)
        wrong_opening = dict(self.opening)
        wrong_opening["boundaries"] = {"reserve_candidates_assigned_to_train_new_only": False}
        with self.assertRaises(ValueError):
            seal(self.packets, self.template, submitted, wrong_opening)


if __name__ == "__main__":
    unittest.main()
