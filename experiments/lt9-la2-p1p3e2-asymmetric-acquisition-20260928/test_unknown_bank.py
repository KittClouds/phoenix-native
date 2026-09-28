"""Invariant tests for synthetic UNKNOWN-bank construction."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from build_unknown_bank import VARIANTS, canonical_json, make_examples, read_fit_bases


class UnknownBankTests(unittest.TestCase):
    def test_only_fit_rows_are_read_and_holdouts_are_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packets_path = root / "packets.json"
            ledger_path = root / "ledger.json"
            packets_path.write_text(
                json.dumps(
                    [
                        {
                            "packet_id": "fit-1",
                            "lexical_relation": ["car", "vehicle"],
                            "orientation": "q-to-d",
                            "query_contexts": ["a [SOURCE]"],
                            "document_contexts": ["a [TARGET]"],
                            "judgment": "THIS FIELD MUST BE IGNORED",
                        },
                        {
                            "packet_id": "holdout-1",
                            "lexical_relation": ["credit", "loan"],
                            "orientation": "q-to-d",
                            "query_contexts": ["sealed [SOURCE]"],
                            "document_contexts": ["sealed [TARGET]"],
                        },
                    ]
                ),
                encoding="utf-8",
            )
            ledger_path.write_text(
                json.dumps(
                    [
                        {"packet_id": "fit-1", "partition": "FIT", "qrels_grade": 999},
                        {"packet_id": "holdout-1", "partition": "HOLDOUT", "qrels_grade": 999},
                    ]
                ),
                encoding="utf-8",
            )
            bases = read_fit_bases("fixture", packets_path, ledger_path)
            self.assertEqual([base["source_packet_id"] for base in bases], ["fit-1"])
            examples, counts = make_examples(bases)
            self.assertEqual(len(examples), len(VARIANTS))
            self.assertEqual(counts["unique_natural_fit_bases"], 1)
            self.assertTrue(all(row["target"] == "UNKNOWN" for row in examples))
            serialized = json.dumps(examples)
            self.assertNotIn("THIS FIELD MUST BE IGNORED", serialized)
            self.assertNotIn("999", serialized)
            self.assertNotIn("sealed", serialized)

    def test_unknown_variants_have_expected_information_erasure(self) -> None:
        base = {
            "lexical_relation": ["car", "vehicle"],
            "orientation": "q-to-d",
            "query_contexts": ["near [SOURCE] context"],
            "document_contexts": ["far [TARGET] context"],
            "source_packet_id": "fit-1",
            "source_key": "fixture",
            "base_content_sha256": "f" * 64,
        }
        examples, _ = make_examples([base])
        by_variant = {row["variant"]: row for row in examples}
        self.assertEqual(by_variant["QUERY_SIDE_ABSENT"]["model_input"]["query_contexts"], [])
        self.assertEqual(by_variant["DOCUMENT_SIDE_ABSENT"]["model_input"]["document_contexts"], [])
        self.assertEqual(by_variant["BOTH_SIDES_ABSENT"]["model_input"], {
            "query_contexts": [], "document_contexts": []
        })
        self.assertEqual(by_variant["BOTH_SIDES_REDACTED"]["model_input"], {
            "query_contexts": ["[CONTEXT_REDACTED]"],
            "document_contexts": ["[CONTEXT_REDACTED]"],
        })
        self.assertEqual(len({row["metadata"]["base_group_id"] for row in examples}), 1)
        self.assertEqual(
            b"\n".join(canonical_json(row) for row in examples),
            b"\n".join(canonical_json(row) for row in make_examples([base])[0]),
        )

    def test_exact_duplicate_context_bases_do_not_multiply_unknown_rows(self) -> None:
        base = {
            "lexical_relation": ["loan", "debt"],
            "orientation": "q-to-d",
            "query_contexts": ["[SOURCE]"],
            "document_contexts": ["[TARGET]"],
            "source_packet_id": "fit-1",
            "source_key": "first",
            "base_content_sha256": "a" * 64,
        }
        duplicate = {**base, "source_packet_id": "fit-2", "source_key": "second"}
        examples, counts = make_examples([base, duplicate])
        self.assertEqual(len(examples), len(VARIANTS))
        self.assertEqual(counts["unique_natural_fit_bases"], 1)
        self.assertEqual(len(examples[0]["metadata"]["source_refs"]), 2)


if __name__ == "__main__":
    unittest.main()
