#!/usr/bin/env python3
"""Update frozen TRAIN support after a sealed reserve review; never fit a model."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


LABELS = ("SAME", "DIFFERENT", "UNKNOWN")
SPLITS = ("TRAIN-NEW", "DEV-NEW", "TEST-NEW")
ALLOWED = set(LABELS)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def read_packet_ids(path: Path) -> list[str]:
    packets = read_json(path)
    if not isinstance(packets, list):
        raise ValueError(f"packet file is not a JSON array: {path}")
    ids = [row.get("packet_id") for row in packets]
    if any(not isinstance(packet_id, str) or not packet_id for packet_id in ids):
        raise ValueError(f"packet file contains a missing packet ID: {path}")
    return ids


def read_ledger_metadata(path: Path) -> list[dict[str, str]]:
    fields = ("packet_id", "base_group_id", "relation", "population", "split")
    rows: list[dict[str, str]] = []
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        raw = json.loads(line)
        row = {field: raw.get(field) for field in fields}
        if any(not isinstance(row[field], str) or not row[field] for field in fields):
            raise ValueError(f"ledger line {line_number}: missing required sampling metadata")
        rows.append(row)
    return rows


def validate_seal(
    canonical: list[dict[str, Any]], receipt: dict[str, Any], expected_ids: list[str], role: str, canonical_path: Path
) -> dict[str, int]:
    if receipt.get("status") != "LABEL_SET_SEALED_COVERAGE_VALIDATED_NOT_FITTED" and receipt.get("status") != "RESERVE_LABELS_SEALED_TRAIN_SUPPORT_ONLY_NOT_FITTED":
        raise ValueError(f"{role}: judgment receipt is not sealed")
    if receipt.get("outputs", {}).get("canonical_judgments_sha256") != sha256(canonical_path):
        raise ValueError(f"{role}: canonical judgment hash differs from its seal")
    ids = [row.get("packet_id") for row in canonical]
    if ids != expected_ids or len(ids) != len(set(ids)):
        raise ValueError(f"{role}: labels do not exactly match frozen packet IDs/order")
    counts: Counter[str] = Counter()
    for row in canonical:
        if set(row) != {"packet_id", "judgment"} or row.get("judgment") not in ALLOWED:
            raise ValueError(f"{role}: malformed canonical judgment for {row.get('packet_id')}")
        counts[row["judgment"]] += 1
    recorded = receipt.get("label_counts", {})
    if any(recorded.get(label) != counts[label] for label in LABELS):
        raise ValueError(f"{role}: label counts differ from the seal receipt")
    return {label: counts[label] for label in LABELS}


def validate_frozen_inputs(primary_dir: Path, reserve_dir: Path) -> None:
    opening = read_json(reserve_dir / "opening-receipt.json")
    expected = opening.get("outputs", {})
    for name, key in (("review-packets.json", "review_packets_sha256"),
                      ("judgments-template.json", "judgments_template_sha256"),
                      ("context-rubric.md", "rubric_sha256")):
        if expected.get(key) != sha256(reserve_dir / name):
            raise ValueError(f"reserve {name} differs from its opening receipt")
    reserve_seal = read_json(reserve_dir / "judgment-seal-receipt.json")
    for input_name, hash_key in (("review-packets.json", "frozen_packets"),
                                 ("judgments-template.json", "frozen_template"),
                                 ("opening-receipt.json", "opening_receipt"),
                                 ("context-rubric.md", "frozen_rubric")):
        bound = reserve_seal.get("inputs", {}).get(hash_key, {}).get("sha256")
        if bound != sha256(reserve_dir / input_name):
            raise ValueError(f"reserve label seal is not bound to current {input_name}")
    primary_receipt = read_json(primary_dir / "judgment-seal-receipt.json")
    primary_labels = primary_dir / "judgments-reviewed.json"
    if primary_receipt.get("outputs", {}).get("canonical_judgments_sha256") != sha256(primary_labels):
        raise ValueError("primary labels differ from their canonical seal")


def assess(primary_dir: Path, reserve_dir: Path, lock_path: Path) -> dict[str, Any]:
    validate_frozen_inputs(primary_dir, reserve_dir)
    ledger_path = primary_dir / "private-candidate-ledger.jsonl"
    ledger = read_ledger_metadata(ledger_path)
    primary_ids = read_packet_ids(primary_dir / "review-packets.json")
    reserve_ids = read_packet_ids(reserve_dir / "review-packets.json")
    primary_labels = read_json(primary_dir / "judgments-reviewed.json")
    reserve_labels = read_json(reserve_dir / "judgments-reviewed.json")
    primary_receipt = read_json(primary_dir / "judgment-seal-receipt.json")
    reserve_receipt = read_json(reserve_dir / "judgment-seal-receipt.json")
    opening = read_json(reserve_dir / "opening-receipt.json")
    lock = read_json(lock_path)

    primary_rows = [row for row in ledger if row["population"] == "PRIMARY"]
    reserve_rows = [row for row in ledger if row["population"] == "RESERVE"]
    primary_ledger_ids = [row["packet_id"] for row in primary_rows]
    opened_reserve_ledger_ids = [
        row["packet_id"] for row in reserve_rows if row["relation"] in set(opening["opened_relations"])
    ]
    if len(primary_ledger_ids) != len(set(primary_ledger_ids)) or set(primary_ledger_ids) != set(primary_ids):
        raise ValueError("primary ledger IDs differ from the frozen primary packet set")
    if len(opened_reserve_ledger_ids) != len(set(opened_reserve_ledger_ids)) or set(opened_reserve_ledger_ids) != set(reserve_ids):
        raise ValueError("opened reserve ledger IDs differ from frozen reserve packets")
    primary_counts = validate_seal(primary_labels, primary_receipt, primary_ids, "primary", primary_dir / "judgments-reviewed.json")
    reserve_counts = validate_seal(reserve_labels, reserve_receipt, reserve_ids, "reserve", reserve_dir / "judgments-reviewed.json")
    if len(primary_rows) != 216 or len(reserve_ids) != 96:
        raise ValueError("frozen primary or opened-reserve population size changed")
    opened_relations = set(opening["opened_relations"])
    opened_reserve_rows = [row for row in reserve_rows if row["relation"] in opened_relations]
    if any(row["split"] != "TRAIN-NEW" for row in opened_reserve_rows):
        raise ValueError("opened reserve includes a non-TRAIN row")

    label_by_id = {row["packet_id"]: row["judgment"] for row in primary_labels + reserve_labels}
    relations = sorted({row["relation"] for row in primary_rows})
    if len(relations) != 9 or set(opening["opened_relations"]) != set(relations) - {"stock->bond"}:
        raise ValueError("reserve opening did not preserve the frozen relation eligibility")

    split_counts: dict[str, dict[str, Counter[str]]] = {
        relation: {split: Counter() for split in SPLITS} for relation in relations
    }
    train_groups: dict[str, dict[str, set[str]]] = {
        relation: {label: set() for label in LABELS} for relation in relations
    }
    for row in primary_rows:
        label = label_by_id[row["packet_id"]]
        split_counts[row["relation"]][row["split"]][label] += 1
        if row["split"] == "TRAIN-NEW":
            train_groups[row["relation"]][label].add(row["base_group_id"])
    for row in opened_reserve_rows:
        label = label_by_id[row["packet_id"]]
        split_counts[row["relation"]]["TRAIN-NEW"][label] += 1
        if row["base_group_id"] in train_groups[row["relation"]][label]:
            raise ValueError(f"duplicate TRAIN base group for {row['relation']} / {label}")
        train_groups[row["relation"]][label].add(row["base_group_id"])

    contract = lock["support_contract"]
    legacy = {
        f"{row['source']}->{row['target']}": {
            "SAME": int(row["legacy_same"]), "DIFFERENT": int(row["legacy_different"])
        }
        for row in lock["relations"]
    }
    result_relations: dict[str, Any] = {}
    for relation in relations:
        train = split_counts[relation]["TRAIN-NEW"]
        totals = {
            label: legacy[relation].get(label, 0) + len(train_groups[relation][label])
            for label in ("SAME", "DIFFERENT")
        }
        passed = (
            totals["SAME"] >= contract["natural_train_same_min"]
            and totals["DIFFERENT"] >= contract["natural_train_different_min"]
            and len(train_groups[relation]["SAME"]) >= contract["train_new_same_min"]
            and len(train_groups[relation]["DIFFERENT"]) >= contract["train_new_different_min"]
        )
        result_relations[relation] = {
            "train_new_counts": {label: train[label] for label in LABELS},
            "dev_new_counts_unchanged": {label: split_counts[relation]["DEV-NEW"][label] for label in LABELS},
            "test_new_counts_unchanged": {label: split_counts[relation]["TEST-NEW"][label] for label in LABELS},
            "train_new_distinct_base_groups": {label: len(train_groups[relation][label]) for label in LABELS},
            "legacy_support": legacy[relation],
            "combined_train_support": totals,
            "support_state": "SUPPORTED" if passed else "UNSUPPORTED_ABSTAIN",
        }

    all_new_labels = Counter(label_by_id.values())
    unknown_relations = {
        row["relation"] for row in primary_rows + opened_reserve_rows
        if label_by_id[row["packet_id"]] == "UNKNOWN"
    }
    unknown_monitor = lock["natural_unknown_monitor"]
    unknown_monitor_pass = (
        all_new_labels["UNKNOWN"] >= unknown_monitor["minimum_count"]
        and len(unknown_relations) >= unknown_monitor["minimum_relations"]
    )
    unopened_reserve_counts = Counter(
        row["relation"] for row in reserve_rows if row["relation"] not in opened_relations
    )
    supported = [relation for relation, row in result_relations.items() if row["support_state"] == "SUPPORTED"]
    unsupported = [relation for relation, row in result_relations.items() if row["support_state"] != "SUPPORTED"]
    return {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2b-reserve-support-update/v1",
        "status": "TRAIN_SUPPORT_UPDATED_NO_FEATURE_JOIN_NO_FIT",
        "reserve_opening_status": opening["status"],
        "counts": {
            "primary_packets": len(primary_ids),
            "opened_reserve_packets": len(reserve_ids),
            "primary_labels": primary_counts,
            "reserve_labels": reserve_counts,
            "combined_new_labels": {label: all_new_labels[label] for label in LABELS},
            "natural_unknown_relations_across_primary_and_opened_reserve": len(unknown_relations),
            "natural_unknown_monitor": {
                "observed_count": all_new_labels["UNKNOWN"],
                "minimum_count": unknown_monitor["minimum_count"],
                "observed_relations": len(unknown_relations),
                "minimum_relations": unknown_monitor["minimum_relations"],
                "passed": unknown_monitor_pass,
            },
        },
        "relations": result_relations,
        "gate": {
            "relations_supported": supported,
            "relations_unsupported": unsupported,
            "all_relations_supported": not unsupported,
            "next_action": (
                "PROCEED_TO_FROZEN_NEXT_GATE" if not unsupported else
                "KEEP_UNSUPPORTED_RELATIONS_ABSTAIN; ANY_NEW_ACQUISITION_REQUIRES_A_NEW_FROZEN_PROTOCOL"
            ),
            "reserve_status": {
                "opened_relations": sorted(opened_relations),
                "unopened_relation_queues": dict(sorted(unopened_reserve_counts.items())),
                "unsupported_relations_with_unopened_reserve": sorted(
                    relation for relation in unsupported if unopened_reserve_counts[relation] > 0
                ),
                "all_reserve_rows_for_unsupported_relations_consumed": all(
                    unopened_reserve_counts[relation] == 0 for relation in unsupported
                ),
            },
        },
        "boundaries": {
            "joined_fields": ["packet_id", "base_group_id", "relation", "population", "split"],
            "context_text_or_feature_fields_joined": False,
            "packet_text_discarded_before_label_metadata_join": True,
            "only_train_new_reserve_labels_added": True,
            "dev_test_labels_used_for_support": False,
            "dev_test_identities_or_assignments_changed": False,
            "stock_bond_reserve_opened": False,
            "synthetic_unknown_counted_as_relation_support": False,
            "model_fit_or_threshold_selection": False,
            "retrieval_or_serving_run": False,
            "retrieval_canaries_opened": False,
        },
        "input_hashes": {
            "primary_judgments": sha256(primary_dir / "judgments-reviewed.json"),
            "primary_judgment_seal": sha256(primary_dir / "judgment-seal-receipt.json"),
            "primary_ledger": sha256(ledger_path),
            "reserve_judgments": sha256(reserve_dir / "judgments-reviewed.json"),
            "reserve_judgment_seal": sha256(reserve_dir / "judgment-seal-receipt.json"),
            "reserve_opening_receipt": sha256(reserve_dir / "opening-receipt.json"),
            "acquisition_lock": sha256(lock_path),
            "assessor_script": sha256(Path(__file__).resolve()),
        },
    }


def render(result: dict[str, Any]) -> str:
    lines = [
        "# P1P3E2-B Reserve TRAIN Support Update",
        "",
        "**Disposition:** sealed reserve labels were joined only to frozen sampling metadata for TRAIN support accounting. No context feature join, fitting, threshold selection, retrieval, or serving occurred.",
        "",
        f"- Primary labels: {result['counts']['primary_packets']} packets; SAME {result['counts']['primary_labels']['SAME']}, DIFFERENT {result['counts']['primary_labels']['DIFFERENT']}, UNKNOWN {result['counts']['primary_labels']['UNKNOWN']}.",
        f"- Opened reserve labels: {result['counts']['opened_reserve_packets']} packets; SAME {result['counts']['reserve_labels']['SAME']}, DIFFERENT {result['counts']['reserve_labels']['DIFFERENT']}, UNKNOWN {result['counts']['reserve_labels']['UNKNOWN']}.",
        f"- Combined new natural labels: SAME {result['counts']['combined_new_labels']['SAME']}, DIFFERENT {result['counts']['combined_new_labels']['DIFFERENT']}, UNKNOWN {result['counts']['combined_new_labels']['UNKNOWN']}.",
        f"- Support contract: {len(result['gate']['relations_supported'])}/9 relations supported; {len(result['gate']['relations_unsupported'])}/9 remain unsupported.",
        f"- Natural UNKNOWN appears across {result['counts']['natural_unknown_relations_across_primary_and_opened_reserve']} relations across primary plus opened reserves.",
        f"- Natural UNKNOWN monitor: {'PASS' if result['counts']['natural_unknown_monitor']['passed'] else 'NOT MET'} ({result['counts']['natural_unknown_monitor']['observed_count']}/{result['counts']['natural_unknown_monitor']['minimum_count']} judgments across {result['counts']['natural_unknown_monitor']['observed_relations']}/{result['counts']['natural_unknown_monitor']['minimum_relations']} relations).",
        "",
        "## Relation support",
        "",
        "| Directed relation | TRAIN-NEW S/D/U | DEV-NEW S/D/U | TEST-NEW S/D/U | New TRAIN groups S/D | Combined TRAIN S/D | State |",
        "|---|---:|---:|---:|---:|---:|---|",
    ]
    for relation, row in result["relations"].items():
        def triplet(key: str) -> str:
            counts = row[key]
            return f"{counts['SAME']}/{counts['DIFFERENT']}/{counts['UNKNOWN']}"
        groups = row["train_new_distinct_base_groups"]
        totals = row["combined_train_support"]
        lines.append(
            f"| `{relation}` | {triplet('train_new_counts')} | {triplet('dev_new_counts_unchanged')} | {triplet('test_new_counts_unchanged')} | {groups['SAME']}/{groups['DIFFERENT']} | {totals['SAME']}/{totals['DIFFERENT']} | {row['support_state']} |"
        )
    lines.extend([
        "",
        "## Boundary and disposition",
        "",
        f"`{result['gate']['next_action']}`. Only the eight pre-authorized relation queues were opened; `stock→bond` reserve rows remain sealed. The remaining six unsupported relations have no unopened rows in their frozen queues. All reserve labels contribute to TRAIN-NEW only. DEV/TEST labels and identities did not contribute to support and were not modified. The support result is engineering data sufficiency only, not model qualification.",
        "",
    ])
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--primary-dir", type=Path, required=True)
    parser.add_argument("--reserve-dir", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("P1P3E2B_ACQUISITION_LOCK.json"))
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    json_path, md_path = args.output_dir / "reserve-support-update.json", args.output_dir / "reserve-support-update.md"
    if json_path.exists() or md_path.exists():
        raise SystemExit(f"refusing to overwrite existing outputs in {args.output_dir}")
    result = assess(args.primary_dir, args.reserve_dir, args.lock)
    json_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    md_path.write_text(render(result), encoding="utf-8", newline="\n")
    print(json.dumps({"status": result["status"], "counts": result["counts"], "gate": result["gate"], "outputs": [str(json_path), str(md_path)]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
