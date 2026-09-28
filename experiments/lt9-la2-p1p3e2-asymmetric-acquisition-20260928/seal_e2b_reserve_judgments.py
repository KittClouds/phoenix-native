#!/usr/bin/env python3
"""Validate and seal reserve review labels without opening private metadata."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ALLOWED = {"SAME", "DIFFERENT", "UNKNOWN"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def seal(
    packets: list[dict[str, Any]],
    template: list[dict[str, Any]],
    submitted: list[dict[str, Any]],
    opening: dict[str, Any],
) -> tuple[list[dict[str, str]], dict[str, Any]]:
    packet_ids = [row.get("packet_id") for row in packets]
    template_ids = [row.get("packet_id") for row in template]
    submitted_ids = [row.get("packet_id") for row in submitted]
    if len(packet_ids) != len(set(packet_ids)):
        raise ValueError("duplicate packet IDs in frozen reserve packets")
    if packet_ids != template_ids:
        raise ValueError("template packet IDs/order differ from frozen reserve packets")
    if packet_ids != submitted_ids:
        raise ValueError("submitted packet IDs/order differ from frozen reserve packets")
    if len(packet_ids) != opening.get("opened_candidate_count"):
        raise ValueError("reserve packet count differs from opening receipt")
    if opening.get("status") != "FROZEN_RESERVES_OPENED_FOR_TRAIN_SUPPORT_ONLY":
        raise ValueError("opening receipt does not authorize reserve training support")
    if not opening.get("boundaries", {}).get("reserve_candidates_assigned_to_train_new_only"):
        raise ValueError("reserve opening receipt does not guarantee TRAIN-NEW assignment")

    canonical: list[dict[str, str]] = []
    counts: Counter[str] = Counter()
    for row in submitted:
        judgment = row.get("judgment")
        if judgment not in ALLOWED:
            raise ValueError(f"invalid judgment {judgment!r} for {row.get('packet_id')}")
        canonical.append({"packet_id": row["packet_id"], "judgment": judgment})
        counts[judgment] += 1
    if len(canonical) != len(packet_ids):
        raise ValueError("every reserve packet must have exactly one judgment")

    return canonical, {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2b-reserve-judgment-seal/v1",
        "sealed_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "RESERVE_LABELS_SEALED_TRAIN_SUPPORT_ONLY_NOT_FITTED",
        "packet_count": len(packet_ids),
        "label_counts": {label: counts[label] for label in ("SAME", "DIFFERENT", "UNKNOWN")},
        "identity_validation": {
            "submitted_ids_exactly_match_frozen_packet_order": True,
            "packet_ids_unique": True,
            "all_packet_ids_present_once": True,
            "judgment_values_valid": True,
            "extra_fields_removed_from_canonical_copy": True,
        },
        "reviewer_provenance": {
            "source": "user-submitted JSON in chat",
            "reviewer_identity": "not stated",
            "reviewer_count": "not established by this submission",
            "review_passes": "one submitted label per packet; no independent second-pass file provided",
            "consensus_or_adjudication": False,
        },
        "scope": {
            "opened_relations": opening.get("opened_relations", []),
            "partition": "TRAIN-NEW only",
            "purpose": "frozen relation-support accounting only",
        },
        "inputs": {},
        "outputs": {},
        "boundaries": {
            "private_ledger_read": False,
            "feature_joined": False,
            "model_fit_or_threshold_selection": False,
            "retrieval_or_serving_run": False,
            "DEV_TEST_labels_opened_or_modified": False,
            "retrieval_canaries_opened": False,
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bank-dir", type=Path, required=True)
    args = parser.parse_args()
    packets_path = args.bank_dir / "review-packets.json"
    template_path = args.bank_dir / "judgments-template.json"
    raw_path = args.bank_dir / "judgments-submitted-raw.json"
    opening_path = args.bank_dir / "opening-receipt.json"
    canonical_path = args.bank_dir / "judgments-reviewed.json"
    receipt_path = args.bank_dir / "judgment-seal-receipt.json"
    if canonical_path.exists() or receipt_path.exists():
        raise SystemExit("refusing to overwrite an existing reserve label seal")

    packets, template = read_json(packets_path), read_json(template_path)
    submitted, opening = read_json(raw_path), read_json(opening_path)
    canonical, receipt = seal(packets, template, submitted, opening)
    args.bank_dir.mkdir(parents=True, exist_ok=True)
    canonical_path.write_text(
        json.dumps(canonical, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n"
    )
    receipt["inputs"] = {
        role: {"path": path.name, "bytes": path.stat().st_size, "sha256": sha256(path)}
        for role, path in {
            "submitted_raw": raw_path,
            "frozen_template": template_path,
            "frozen_packets": packets_path,
            "opening_receipt": opening_path,
            "frozen_rubric": args.bank_dir / "context-rubric.md",
        }.items()
    }
    receipt["outputs"] = {
        "canonical_judgments": canonical_path.name,
        "canonical_judgments_sha256": sha256(canonical_path),
    }
    receipt["sealer"] = {"path": str(Path(__file__).resolve()), "sha256": sha256(Path(__file__))}
    receipt_path.write_text(
        json.dumps(receipt, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n"
    )
    print(json.dumps({"status": receipt["status"], "packet_count": len(canonical), "label_counts": receipt["label_counts"], "receipt": str(receipt_path)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
