#!/usr/bin/env python3
"""Seal a model-origin A1 label pass without reading the private ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter, defaultdict
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--packets", type=Path, required=True)
    parser.add_argument("--rubric", type=Path, required=True)
    parser.add_argument("--template", type=Path, required=True)
    parser.add_argument("--acquisition-receipt", type=Path, required=True)
    parser.add_argument("--judgments", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if args.output.exists():
        raise SystemExit(f"refusing to overwrite existing seal: {args.output}")

    packets = read_json(args.packets)
    template = read_json(args.template)
    judgments = read_json(args.judgments)
    acquisition = read_json(args.acquisition_receipt)
    if not all(isinstance(rows, list) for rows in (packets, template, judgments)):
        raise ValueError("packets, template, and judgments must be JSON arrays")

    packet_ids = [row["packet_id"] for row in packets]
    template_ids = [row["packet_id"] for row in template]
    judgment_ids = [row["packet_id"] for row in judgments]
    if len(packet_ids) != len(set(packet_ids)):
        raise ValueError("duplicate packet IDs in visible packet set")
    if packet_ids != template_ids:
        raise ValueError("packet IDs/order differ from the frozen blank template")
    if packet_ids != judgment_ids:
        raise ValueError("judgment IDs/order differ from frozen packet order")
    if len(packet_ids) != acquisition.get("packet_count"):
        raise ValueError("packet count differs from acquisition receipt")

    by_id = {row["packet_id"]: row for row in packets}
    global_counts: Counter[str] = Counter()
    relation_counts: dict[str, Counter[str]] = defaultdict(Counter)
    for row in judgments:
        label = row.get("judgment")
        if label not in ALLOWED:
            raise ValueError(f"invalid judgment {label!r} for {row['packet_id']}")
        packet = by_id[row["packet_id"]]
        relation = "->".join(packet["lexical_relation"])
        global_counts[label] += 1
        relation_counts[relation][label] += 1

    if sum(global_counts.values()) != len(packet_ids):
        raise ValueError("not every packet has exactly one judgment")

    bound_files = {
        "review_packets": args.packets,
        "rubric": args.rubric,
        "blank_template": args.template,
        "acquisition_receipt": args.acquisition_receipt,
        "model_origin_judgments": args.judgments,
    }
    receipt = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2a1-proxy-label-seal/v1",
        "sealed_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "SEALED_DISCOVERY_LABELS_MODEL_ORIGIN_NOT_INDEPENDENT_HUMAN_REVIEW",
        "reviewer_provenance": {
            "kind": "assistant_model_single_pass",
            "reviewer": "OpenAI Codex model-origin proxy pass",
            "independent_human_review": False,
            "human_judgments": False,
            "judgment_basis": "visible masked A1 packets and frozen rubric, interpreted with the prior conversation context",
        },
        "exposure_and_firewall": {
            "prior_experiment_hypotheses_and_relation_level_results_in_conversation": True,
            "aggregate_acquisition_receipt_seen_before_review": True,
            "aggregate_receipt_disclosed_fit_opportunities": acquisition.get("opportunity_count"),
            "aggregate_receipt_disclosed_top10_controls": acquisition.get("top10_control_count"),
            "A1_item_level_judgments_seen_before_this_pass": False,
            "packet_level_qrels_rank_or_opportunity_status_visible": False,
            "private_ledger_opened_before_label_seal": False,
            "row_level_qrels_or_rank_used_for_judgments": False,
            "judgments_template_modified": False,
        },
        "sample_scope": {
            "packet_count": len(packet_ids),
            "partition": "FIT_ONLY",
            "retrieval_holdouts_in_packet_set": False,
            "interpretation": "discovery-only proxy labels; not independent-human validation or qualification",
        },
        "counts": {
            "overall": {label: global_counts[label] for label in sorted(ALLOWED)},
            "by_directed_relation": {
                relation: {label: counts[label] for label in sorted(ALLOWED)}
                for relation, counts in sorted(relation_counts.items())
            },
        },
        "packet_order_sha256": hashlib.sha256(
            json.dumps(packet_ids, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        ).hexdigest(),
        "sealer_script": {"path": str(Path(__file__).resolve()), "sha256": sha256(Path(__file__))},
        "inputs": {
            role: {"path": str(path), "bytes": path.stat().st_size, "sha256": sha256(path)}
            for role, path in bound_files.items()
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps({"status": receipt["status"], "counts": receipt["counts"]["overall"], "output": str(args.output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
