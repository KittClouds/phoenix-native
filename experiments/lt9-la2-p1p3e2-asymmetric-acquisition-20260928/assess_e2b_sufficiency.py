#!/usr/bin/env python3
"""Label/split sufficiency gate for P1P3E2-B; no context feature join or fitting."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


LABELS = ("SAME", "DIFFERENT", "UNKNOWN")
SPLITS = ("TRAIN-NEW", "DEV-NEW", "TEST-NEW")
LANES = ("SEMANTIC_NEAR", "SENSE_CONTRAST", "SPARSE_OR_BOUNDARY")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_ledger_metadata(path: Path) -> list[dict[str, str]]:
    """Read only sampling metadata; context and feature fields are discarded."""
    metadata_fields = ("packet_id", "base_group_id", "relation", "population", "split", "lane", "dataset")
    rows: list[dict[str, str]] = []
    with path.open("r", encoding="utf-8") as stream:
        for line_number, line in enumerate(stream, start=1):
            if not line.strip():
                continue
            raw = json.loads(line)
            row = {key: raw.get(key) for key in metadata_fields}
            if any(not isinstance(row[key], str) or not row[key] for key in metadata_fields):
                raise ValueError(f"ledger line {line_number}: missing required metadata")
            rows.append(row)  # Never retain context text or other payload fields.
    return rows


def assess(labels: list[dict[str, Any]], ledger: list[dict[str, str]], lock: dict[str, Any]) -> dict[str, Any]:
    label_ids = [row.get("packet_id") for row in labels]
    if len(label_ids) != len(set(label_ids)):
        raise ValueError("duplicate packet IDs in labels")
    if any(row.get("judgment") not in LABELS for row in labels):
        raise ValueError("invalid or missing judgment")

    primary = [row for row in ledger if row["population"] == "PRIMARY"]
    reserves = [row for row in ledger if row["population"] == "RESERVE"]
    primary_ids = [row["packet_id"] for row in primary]
    if len(primary_ids) != len(set(primary_ids)):
        raise ValueError("duplicate primary packet IDs in ledger")
    if set(label_ids) != set(primary_ids):
        raise ValueError("labels do not cover exactly the primary packet set")
    if len(labels) != 216 or len(primary) != 216 or len(reserves) != 108:
        raise ValueError("frozen primary/reserve population sizes changed")

    label_by_id = {row["packet_id"]: row["judgment"] for row in labels}
    primary_groups = [row["base_group_id"] for row in primary]
    if len(primary_groups) != len(set(primary_groups)):
        raise ValueError("primary base groups are not unique")

    counts: dict[str, dict[str, Counter[str]]] = defaultdict(lambda: {split: Counter() for split in SPLITS})
    lane_counts: dict[str, dict[str, dict[str, Counter[str]]]] = defaultdict(
        lambda: {split: {lane: Counter() for lane in LANES} for split in SPLITS}
    )
    dataset_counts: dict[str, Counter[str]] = defaultdict(Counter)
    groups: dict[str, dict[str, set[str]]] = defaultdict(lambda: {label: set() for label in LABELS})
    for row in primary:
        relation, split, lane = row["relation"], row["split"], row["lane"]
        if split not in SPLITS or lane not in LANES:
            raise ValueError(f"unexpected frozen split/lane: {split!r}/{lane!r}")
        label = label_by_id[row["packet_id"]]
        counts[relation][split][label] += 1
        lane_counts[relation][split][lane][label] += 1
        dataset_counts[relation][row["dataset"]] += 1
        groups[relation][label].add(row["base_group_id"])

    contract = lock["support_contract"]
    legacy = {
        f"{row['source']}->{row['target']}": {
            "SAME": int(row["legacy_same"]),
            "DIFFERENT": int(row["legacy_different"]),
        }
        for row in lock["relations"]
    }
    relations: dict[str, Any] = {}
    for relation in sorted(counts):
        train = counts[relation]["TRAIN-NEW"]
        total_same = legacy[relation]["SAME"] + train["SAME"]
        total_different = legacy[relation]["DIFFERENT"] + train["DIFFERENT"]
        supported = (
            total_same >= contract["natural_train_same_min"]
            and total_different >= contract["natural_train_different_min"]
            and train["SAME"] >= contract["train_new_same_min"]
            and train["DIFFERENT"] >= contract["train_new_different_min"]
        )
        relations[relation] = {
            "new_counts_by_split": {
                split: {label: counts[relation][split][label] for label in LABELS}
                for split in SPLITS
            },
            "train_new_groups": {label: len(groups[relation][label]) for label in LABELS},
            "legacy_train_support": legacy[relation],
            "combined_train_support": {"SAME": total_same, "DIFFERENT": total_different},
            "support_state": "SUPPORTED" if supported else "UNSUPPORTED_ABSTAIN",
            "natural_unknown_new_total": sum(counts[relation][split]["UNKNOWN"] for split in SPLITS),
            "natural_unknown_in_sparse_lane": sum(
                lane_counts[relation][split]["SPARSE_OR_BOUNDARY"]["UNKNOWN"] for split in SPLITS
            ),
            "different_in_semantic_near_lane": sum(
                lane_counts[relation][split]["SEMANTIC_NEAR"]["DIFFERENT"] for split in SPLITS
            ),
            "lane_counts_by_split": {
                split: {
                    lane: {label: lane_counts[relation][split][lane][label] for label in LABELS}
                    for lane in LANES
                }
                for split in SPLITS
            },
            "dataset_counts": dict(sorted(dataset_counts[relation].items())),
        }

    total_label_counts = Counter(label_by_id.values())
    unknown_relations = [
        relation for relation, data in relations.items() if data["natural_unknown_new_total"] > 0
    ]
    train_support_pass = all(data["support_state"] == "SUPPORTED" for data in relations.values())
    return {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2b-sufficiency-gate/v1",
        "status": "SUPPORT_GATE_COMPLETE_NO_FEATURE_JOIN_NO_FIT",
        "counts": {
            "primary_packets": len(primary),
            "reserve_candidates_unopened": len(reserves),
            "labels": {label: total_label_counts[label] for label in LABELS},
            "relations": len(relations),
            "distinct_new_natural_unknown_relations": len(unknown_relations),
        },
        "relations": relations,
        "coverage_diagnostics": {
            "semantic_near_different_groups": sum(d["different_in_semantic_near_lane"] for d in relations.values()),
            "relations_with_semantic_near_different": sum(d["different_in_semantic_near_lane"] > 0 for d in relations.values()),
            "sparse_lane_unknown_groups": sum(d["natural_unknown_in_sparse_lane"] for d in relations.values()),
            "relations_with_sparse_lane_unknown": sum(d["natural_unknown_in_sparse_lane"] > 0 for d in relations.values()),
            "natural_unknown_monitor_minimum": lock["natural_unknown_monitor"],
            "natural_unknown_monitor_pass": (
                total_label_counts["UNKNOWN"] >= lock["natural_unknown_monitor"]["minimum_count"]
                and len(unknown_relations) >= lock["natural_unknown_monitor"]["minimum_relations"]
            ),
        },
        "gate": {
            "all_relations_meet_frozen_train_support_contract": train_support_pass,
            "relations_supported": [r for r, d in relations.items() if d["support_state"] == "SUPPORTED"],
            "relations_unsupported": [r for r, d in relations.items() if d["support_state"] != "SUPPORTED"],
            "next_action": "OPEN_FROZEN_RESERVES_FOR_UNSUPPORTED_RELATIONS" if not train_support_pass else "PROCEED_TO_NEXT_PREREGISTERED_GATE",
        },
        "boundaries": {
            "only_packet_id_base_group_relation_split_lane_population_dataset_metadata_joined": True,
            "context_text_or_model_features_joined": False,
            "reserve_rows_labeled_or_opened": False,
            "nontrain_labels_contribute_to_relation_support": False,
            "synthetic_unknown_contributes_to_relation_support": False,
            "model_fit_or_threshold_selection": False,
            "retrieval_or_serving_run": False,
            "holdout_retrieval_canaries_read": False,
        },
    }


def render_report(result: dict[str, Any]) -> str:
    lines = [
        "# P1P3E2-B Primary Review Sufficiency Gate",
        "",
        "**Disposition:** labels and frozen sampling metadata were joined for sufficiency accounting only. No context feature join, model fit, threshold selection, retrieval run, or serving change occurred.",
        "",
        f"- Primary rows: {result['counts']['primary_packets']} across {result['counts']['relations']} directed relations.",
        f"- Submitted labels: SAME {result['counts']['labels']['SAME']}, DIFFERENT {result['counts']['labels']['DIFFERENT']}, UNKNOWN {result['counts']['labels']['UNKNOWN']}.",
        f"- TRAIN support contract: {len(result['gate']['relations_supported'])}/9 relations supported; {len(result['gate']['relations_unsupported'])}/9 remain unsupported.",
        f"- Natural UNKNOWN monitor: {'PASS' if result['coverage_diagnostics']['natural_unknown_monitor_pass'] else 'NOT MET'}.",
        "",
        "## Per-relation split and support counts",
        "",
        "| Directed relation | TRAIN-NEW S/D/U | DEV-NEW S/D/U | TEST-NEW S/D/U | Combined TRAIN S/D | Support |",
        "|---|---:|---:|---:|---:|---|",
    ]
    for relation, data in result["relations"].items():
        def triplet(split: str) -> str:
            c = data["new_counts_by_split"][split]
            return f"{c['SAME']}/{c['DIFFERENT']}/{c['UNKNOWN']}"
        combined = data["combined_train_support"]
        lines.append(
            f"| `{relation}` | {triplet('TRAIN-NEW')} | {triplet('DEV-NEW')} | {triplet('TEST-NEW')} | {combined['SAME']}/{combined['DIFFERENT']} | {data['support_state']} |"
        )
    lines.extend([
        "",
        "## Coverage diagnostics",
        "",
        f"- DIFFERENT labels in the high-overlap `SEMANTIC_NEAR` lane: {result['coverage_diagnostics']['semantic_near_different_groups']} across {result['coverage_diagnostics']['relations_with_semantic_near_different']} relations.",
        f"- UNKNOWN labels in `SPARSE_OR_BOUNDARY`: {result['coverage_diagnostics']['sparse_lane_unknown_groups']} across {result['coverage_diagnostics']['relations_with_sparse_lane_unknown']} relations.",
        f"- Explicit natural UNKNOWN: {result['counts']['labels']['UNKNOWN']} across {result['counts']['distinct_new_natural_unknown_relations']} relations; monitor requires at least {result['coverage_diagnostics']['natural_unknown_monitor_minimum']['minimum_count']} across {result['coverage_diagnostics']['natural_unknown_monitor_minimum']['minimum_relations']} relations.",
        "- DEV-NEW and TEST-NEW class presence is shown per relation in the table; these labels do not contribute to support state.",
        "",
        "## Disposition",
        "",
        f"`{result['gate']['next_action']}`. Only relations missing the frozen training support contract may draw from the preordered reserve queue. The reserve labels remain unopened. Reviewer provenance is recorded in the external judgment seal receipt; this report does not claim independent review or consensus.",
        "",
    ])
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bank-dir", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("P1P3E2B_ACQUISITION_LOCK.json"))
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    output_json = args.output_dir / "sufficiency-gate.json"
    output_md = args.output_dir / "sufficiency-gate.md"
    if output_json.exists() or output_md.exists():
        raise SystemExit(f"refusing to overwrite existing sufficiency outputs in {args.output_dir}")

    labels_path = args.bank_dir / "judgments-reviewed.json"
    ledger_path = args.bank_dir / "private-candidate-ledger.jsonl"
    lock_path = args.lock
    labels = json.loads(labels_path.read_text(encoding="utf-8"))
    ledger = load_ledger_metadata(ledger_path)
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    result = assess(labels, ledger, lock)
    result["input_hashes"] = {
        "judgments": sha256(labels_path),
        "private_ledger": sha256(ledger_path),
        "acquisition_lock": sha256(lock_path),
        "assessor_script": sha256(Path(__file__).resolve()),
    }
    output_json.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    output_md.write_text(render_report(result), encoding="utf-8", newline="\n")
    print(json.dumps({"status": result["status"], "counts": result["counts"], "gate": result["gate"], "outputs": [str(output_json), str(output_md)]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
