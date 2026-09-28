#!/usr/bin/env python3
"""Apply only the frozen DEV-EXT class floors; do not inspect TEST labels."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
DEFAULT_BANK = Path(r"D:\phoenix-evals\lt9-la2-p1p3e2c-extension-20260928\p1p3e2c-extension-v1")
RELATIONS = ("engine->motor", "insurance->coverage", "stock->bond")
FLOORS = {"SAME": 8, "DIFFERENT": 8}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def evaluate(bank: Path) -> dict[str, Any]:
    packets_path = bank / "review-packets.json"
    ledger_path = bank / "private-candidate-ledger.jsonl"
    labels_path = bank / "judgments-reviewed.json"
    seal_path = bank / "judgment-seal-receipt.json"
    out_json = bank / "dev-sufficiency-gate.json"
    out_md = bank / "dev-sufficiency-gate.md"
    if out_json.exists() or out_md.exists():
        raise FileExistsError("refusing to overwrite an existing DEV sufficiency result")

    packets = read_json(packets_path)
    labels = read_json(labels_path)
    seal = read_json(seal_path)
    packet_ids = [row["packet_id"] for row in packets]
    if len(packet_ids) != 108 or len(set(packet_ids)) != 108:
        raise ValueError("frozen packet set must contain 108 unique rows")
    if seal.get("status") != "LABELS_SEALED_VALIDATED_NOT_JOINED" or seal.get("packet_count") != 108:
        raise ValueError("validated label seal is missing or has the wrong packet count")
    if seal.get("inputs", {}).get("frozen_packets", {}).get("sha256") != sha256(packets_path):
        raise ValueError("frozen packets differ from label seal")
    if seal.get("outputs", {}).get("sha256") != sha256(labels_path):
        raise ValueError("canonical labels differ from label seal")

    ledger: list[dict[str, Any]] = []
    with ledger_path.open("r", encoding="utf-8") as stream:
        for line in stream:
            if line.strip():
                ledger.append(json.loads(line))
    if [row.get("packet_id") for row in ledger] != packet_ids:
        raise ValueError("private ledger IDs/order differ from frozen packet IDs")
    def relation_key(value: Any) -> str:
        if isinstance(value, str):
            return value
        if isinstance(value, list) and len(value) == 2 and all(isinstance(part, str) for part in value):
            return "->".join(value)
        raise ValueError(f"malformed relation value: {value!r}")

    public_relation = {row["packet_id"]: relation_key(row["lexical_relation"]) for row in packets}
    dev_meta = {row["packet_id"]: row for row in ledger if row.get("split") == "DEV-EXT"}
    if len(dev_meta) != 48:
        raise ValueError(f"frozen DEV-EXT population is not 48 rows: {len(dev_meta)}")

    # Only DEV labels are joined to private partition metadata by this gate check. TEST rows remain unjoined and unscored.
    dev_labels: dict[str, str] = {}
    for row in labels:
        packet_id = row.get("packet_id")
        if packet_id not in dev_meta:
            continue
        if set(row) != {"packet_id", "judgment"}:
            raise ValueError(f"unexpected label fields on DEV row {packet_id}")
        label = row.get("judgment")
        if label not in {"SAME", "DIFFERENT", "UNKNOWN"}:
            raise ValueError(f"invalid DEV label {label!r} for {packet_id}")
        dev_labels[packet_id] = label
    if set(dev_labels) != set(dev_meta):
        raise ValueError("DEV-EXT labels do not map exactly to its 48 frozen packets")

    by_relation: dict[str, Counter[str]] = defaultdict(Counter)
    bases_by_relation: dict[str, dict[str, set[str]]] = defaultdict(lambda: defaultdict(set))
    for packet_id, label in dev_labels.items():
        meta = dev_meta[packet_id]
        relation = public_relation[packet_id]
        if relation != meta.get("relation"):
            raise ValueError(f"review-packet relation differs from private ledger for {packet_id}")
        base = str(meta.get("base_group_id", ""))
        if not base:
            raise ValueError(f"missing base_group_id for {packet_id}")
        by_relation[relation][label] += 1
        bases_by_relation[relation][label].add(base)
    if set(by_relation) != set(RELATIONS):
        raise ValueError("DEV-EXT relation set differs from frozen supported relations")
    for relation, groups in bases_by_relation.items():
        all_bases = [base for values in groups.values() for base in values]
        if len(all_bases) != len(set(all_bases)):
            raise ValueError(f"multiple DEV labels share a base_group_id for {relation}")

    relation_results: dict[str, Any] = {}
    all_pass = True
    for relation in RELATIONS:
        counts = by_relation[relation]
        same_ok = counts["SAME"] >= FLOORS["SAME"]
        different_ok = counts["DIFFERENT"] >= FLOORS["DIFFERENT"]
        passed = same_ok and different_ok
        all_pass &= passed
        relation_results[relation] = {
            "base_groups": sum(counts.values()),
            "labels": {name: counts[name] for name in ("SAME", "DIFFERENT", "UNKNOWN")},
            "floors": {name: FLOORS[name] for name in FLOORS},
            "same_floor_met": same_ok,
            "different_floor_met": different_ok,
            "status": "PASS" if passed else "UNDERPOWERED",
        }

    status = "DEV_EXT_FLOORS_PASS_FEATURE_JOIN_FIT_MAY_PROCEED" if all_pass else "STOP_BEFORE_FEATURE_JOIN_OR_FIT_DEV_UNDERPOWERED"
    result = {
        "schema": "phoenix.lt9.la2.p1p3e2c-dev-sufficiency-gate.v1",
        "evaluated_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": status,
        "partition": "DEV-EXT",
        "unit": "unique base_group_id",
        "relation_results": relation_results,
        "pooled_labels": {name: sum(counts[name] for counts in by_relation.values()) for name in ("SAME", "DIFFERENT", "UNKNOWN")},
        "requirements": {"per_relation_minimum": FLOORS, "exact_packet_count": 48, "no_backfill": True},
        "inputs": {
            name: {"path": str(path), "bytes": path.stat().st_size, "sha256": sha256(path)}
            for name, path in {
                "frozen_packets": packets_path,
                "private_partition_ledger": ledger_path,
                "canonical_submitted_labels": labels_path,
                "label_seal_receipt": seal_path,
            }.items()
        },
        "assessor": {"path": str(Path(__file__).resolve()), "sha256": sha256(Path(__file__))},
        "boundaries": {
            "DEV_EXT_labels_joined_to_partition_metadata": True,
            "TEST_EXT_labels_joined_to_partition_or_scored": False,
            "all_108_submitted_labels_present_in_sealed_input": True,
            "retrieval_canaries_opened": False,
            "feature_vectors_materialized": False,
            "models_fit_or_contacted": False,
            "thresholds_selected": False,
            "retrieval_or_authority_run": False,
        },
    }
    out_json.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    lines = [
        "# P1P3E2-C DEV-EXT sufficiency gate",
        "",
        f"**Disposition:** `{status}`",
        "",
        "This is the preregistered DEV-EXT sufficiency check. The user supplied all 108 judgments in one ordered file, so TEST-EXT labels are present in the sealed input; this check joined only DEV-EXT rows to private partition metadata and did not score TEST-EXT. No feature vectors or model inputs were created.",
        "",
        "| Relation | SAME | DIFFERENT | UNKNOWN | Required SAME/DIFFERENT | Status |",
        "|---|---:|---:|---:|---:|---|",
    ]
    for relation, item in relation_results.items():
        labels_for_relation = item["labels"]
        lines.append(f"| `{relation}` | {labels_for_relation['SAME']} | {labels_for_relation['DIFFERENT']} | {labels_for_relation['UNKNOWN']} | 8 / 8 | {item['status']} |")
    lines += [
        "",
        f"Pooled DEV-EXT labels: {result['pooled_labels']}",
        "",
        "Per frozen protocol, any missed DEV-EXT class floor stops the run before feature joining and model fitting. There is no backfill or additional review packet. TEST predictions were not generated because this gate stopped execution.",
        "",
    ]
    out_md.write_text("\n".join(lines), encoding="utf-8", newline="\n")
    result["report"] = out_md.name
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bank-dir", type=Path, default=DEFAULT_BANK)
    args = parser.parse_args()
    try:
        result = evaluate(args.bank_dir)
    except Exception as exc:
        raise SystemExit(f"P1P3E2-C DEV sufficiency failed closed: {exc}") from exc
    print(json.dumps({"status": result["status"], "relation_results": result["relation_results"], "report": str(args.bank_dir / "dev-sufficiency-gate.md")}, indent=2))
    return 0 if result["status"] == "DEV_EXT_FLOORS_PASS_FEATURE_JOIN_FIT_MAY_PROCEED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
