#!/usr/bin/env python3
"""Validate and seal submitted P1P3E2-C extension labels without opening the private ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_BANK = Path(r"D:\phoenix-evals\lt9-la2-p1p3e2c-extension-20260928\p1p3e2c-extension-v1")
ALLOWED = {"SAME", "DIFFERENT", "UNKNOWN"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def seal(bank: Path) -> tuple[list[dict[str, str]], dict[str, Any]]:
    packets_path = bank / "review-packets.json"
    template_path = bank / "judgments-template.json"
    submitted_path = bank / "judgments-submitted.json"
    receipt_path = bank / "acquisition-receipt.json"
    rubric_path = HERE / "context-rubric.md"
    canonical_path = bank / "judgments-reviewed.json"
    seal_path = bank / "judgment-seal-receipt.json"
    if canonical_path.exists() or seal_path.exists():
        raise FileExistsError("refusing to overwrite an existing P1P3E2-C label seal")

    packets, template, submitted = (read_json(p) for p in (packets_path, template_path, submitted_path))
    acquisition = read_json(receipt_path)
    if not all(isinstance(rows, list) for rows in (packets, template, submitted)):
        raise ValueError("packets, template, and submitted judgments must be JSON arrays")
    packet_ids = [row.get("packet_id") for row in packets]
    template_ids = [row.get("packet_id") for row in template]
    submitted_ids = [row.get("packet_id") for row in submitted]
    if len(packet_ids) != 108 or len(set(packet_ids)) != 108:
        raise ValueError("frozen packet IDs must contain 108 unique rows")
    if packet_ids != template_ids:
        raise ValueError("template IDs/order differ from frozen packet IDs")
    if packet_ids != submitted_ids:
        raise ValueError("submitted IDs/order differ from frozen packet IDs")
    if any(row.get("judgment") is not None for row in template):
        raise ValueError("frozen judgment template is not blank")
    if acquisition.get("status") != "ACQUISITION_COMPLETE_LABELS_UNOPENED":
        raise ValueError("acquisition receipt does not certify complete frozen acquisition")
    expected = {
        "review-packets.json": acquisition.get("review-packets.json", {}),
        "judgments-template.json": acquisition.get("judgments-template.json", {}),
        "private-candidate-ledger.jsonl": acquisition.get("private-candidate-ledger.jsonl", {}),
    }
    for name, metadata in expected.items():
        path = bank / name
        raw = path.read_bytes()
        if len(raw) != metadata.get("bytes") or hashlib.sha256(raw).hexdigest() != metadata.get("sha256"):
            raise ValueError(f"acquisition receipt hash mismatch: {name}")

    canonical: list[dict[str, str]] = []
    counts: Counter[str] = Counter()
    for row in submitted:
        if set(row) != {"packet_id", "judgment"}:
            raise ValueError(f"unexpected fields for {row.get('packet_id')}")
        label = row.get("judgment")
        if label not in ALLOWED:
            raise ValueError(f"invalid judgment {label!r} for {row.get('packet_id')}")
        canonical.append({"packet_id": row["packet_id"], "judgment": label})
        counts[label] += 1

    canonical_path.write_text(json.dumps(canonical, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    inputs = {
        "submitted_labels": submitted_path,
        "frozen_packets": packets_path,
        "frozen_blank_template": template_path,
        "acquisition_receipt": receipt_path,
        "frozen_rubric": rubric_path,
    }
    output = {
        "schema": "phoenix.lt9.la2.p1p3e2c-extension-judgment-seal.v1",
        "sealed_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "LABELS_SEALED_VALIDATED_NOT_JOINED",
        "packet_count": len(canonical),
        "label_counts": {label: counts[label] for label in ("SAME", "DIFFERENT", "UNKNOWN")},
        "reviewer_provenance": {
            "source": "user-submitted JSON from chat",
            "reviewer_identity": "not stated",
            "reviewer_count": "not established by the submission",
            "review_passes": "one submitted judgment per packet; independent replication not established",
            "consensus_or_adjudication": False,
        },
        "identity_validation": {
            "ids_and_order_match_frozen_packets": True,
            "packet_ids_unique": True,
            "every_packet_labeled_once": True,
            "values_in_frozen_vocabulary": True,
            "extra_fields_absent": True,
        },
        "inputs": {
            role: {"path": str(path), "bytes": path.stat().st_size, "sha256": sha256(path)}
            for role, path in inputs.items()
        },
        "outputs": {"canonical_judgments": canonical_path.name, "sha256": sha256(canonical_path)},
        "sealer": {"path": str(Path(__file__).resolve()), "sha256": sha256(Path(__file__))},
        "boundaries": {
            "private_ledger_opened": False,
            "partition_or_relation_labels_joined": False,
            "features_materialized": False,
            "models_fit_or_contacted": False,
            "test_predictions_created": False,
            "retrieval_authority_or_serving_run": False,
        },
    }
    seal_path.write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    return canonical, output


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bank-dir", type=Path, default=DEFAULT_BANK)
    args = parser.parse_args()
    try:
        canonical, receipt = seal(args.bank_dir)
    except Exception as exc:
        raise SystemExit(f"P1P3E2-C label sealing failed closed: {exc}") from exc
    print(json.dumps({"status": receipt["status"], "packet_count": len(canonical), "label_counts": receipt["label_counts"], "receipt": str(args.bank_dir / 'judgment-seal-receipt.json')}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
