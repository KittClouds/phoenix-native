#!/usr/bin/env python3
"""Open only frozen TRAIN reserves for relations failing the E2-B support gate."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import Counter
from pathlib import Path
from typing import Any


TOKEN_RE = re.compile(r"[A-Za-z0-9]+")
EXPECTED_RELATIONS = 9


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def make_packets(gate: dict[str, Any], ledger: list[dict[str, Any]], lock: dict[str, Any]) -> tuple[list[dict[str, str]], dict[str, Any]]:
    if gate.get("status") != "SUPPORT_GATE_COMPLETE_NO_FEATURE_JOIN_NO_FIT":
        raise ValueError("reserve opening requires a completed support-only gate")
    if gate.get("gate", {}).get("next_action") != "OPEN_FROZEN_RESERVES_FOR_UNSUPPORTED_RELATIONS":
        raise ValueError("the frozen support gate did not authorize reserve opening")
    unsupported = set(gate["gate"]["relations_unsupported"])
    if not unsupported or len(gate["relations"]) != EXPECTED_RELATIONS:
        raise ValueError("unexpected relation roster or empty reserve opening set")
    primary = [row for row in ledger if row.get("population") == "PRIMARY"]
    reserves = [row for row in ledger if row.get("population") == "RESERVE" and row.get("relation") in unsupported]
    if len(primary) != 216:
        raise ValueError("primary bank size changed")
    if any(row.get("split") != "TRAIN-NEW" for row in reserves):
        raise ValueError("a reserve is not preassigned to TRAIN-NEW")
    counts = Counter(row["relation"] for row in reserves)
    if counts != Counter({relation: 12 for relation in sorted(unsupported)}):
        raise ValueError("eligible relation reserve queues are incomplete or overfull")
    reserve_ids = [row["packet_id"] for row in reserves]
    if len(reserve_ids) != len(set(reserve_ids)):
        raise ValueError("duplicate reserve packet IDs")

    dev_test_query_ids = {
        (row["dataset"], row["query_id"])
        for row in primary if row.get("split") in {"DEV-NEW", "TEST-NEW"}
    }
    dev_test_doc_ids = {
        (row["dataset"], row["document_id"])
        for row in primary if row.get("split") in {"DEV-NEW", "TEST-NEW"}
    }
    if any((row["dataset"], row["query_id"]) in dev_test_query_ids for row in reserves):
        raise ValueError("reserve query identity overlaps DEV/TEST")
    if any((row["dataset"], row["document_id"]) in dev_test_doc_ids for row in reserves):
        raise ValueError("reserve document identity overlaps DEV/TEST")

    packets: list[dict[str, str]] = []
    for row in reserves:
        relation = row["relation"]
        source, target = relation.split("->", maxsplit=1)
        query_context = row.get("query_context")
        document_context = row.get("document_context")
        if not isinstance(query_context, str) or not isinstance(document_context, str):
            raise ValueError("reserve context missing")
        for text in (query_context, document_context):
            tokens = {token.casefold() for token in TOKEN_RE.findall(text)}
            if source.casefold() in tokens or target.casefold() in tokens:
                raise ValueError(f"candidate form leaked in reserve packet {row['packet_id']}")
        packets.append({
            "packet_id": row["packet_id"],
            "lexical_relation": relation,
            "query_context": query_context,
            "document_context": document_context,
        })

    seed = str(lock["seed"])
    packets.sort(key=lambda p: hashlib.sha256(
        f"{seed}\x1fRESERVE_REVIEW_ORDER\x1f{p['packet_id']}".encode("utf-8")
    ).hexdigest())
    receipt = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2b-reserve-opening/v1",
        "status": "FROZEN_RESERVES_OPENED_FOR_TRAIN_SUPPORT_ONLY",
        "opening_rule": "Open all pre-frozen 12-row reserve queues only for relations marked unsupported by the sealed support gate; preserve TRAIN-NEW assignment; never alter DEV/TEST.",
        "opened_relations": sorted(unsupported),
        "opened_candidate_count": len(packets),
        "per_relation_counts": dict(sorted(counts.items())),
        "per_relation_lane_counts": {
            relation: dict(sorted(Counter(row["lane"] for row in reserves if row["relation"] == relation).items()))
            for relation in sorted(unsupported)
        },
        "order": "deterministic seeded SHA-256 packet order for blind review; source queue identities remain bound to the frozen private ledger",
        "boundaries": {
            "supported_relation_reserves_opened": False,
            "reserve_candidates_selected_using_labels_or_features": False,
            "reserve_candidates_assigned_to_train_new_only": True,
            "review_packets_expose_corpus_split_lane_or_retrieval_metadata": False,
            "model_fit_or_retrieval_run": False,
        },
    }
    return packets, receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bank-dir", type=Path, required=True)
    parser.add_argument("--gate-json", type=Path, required=True)
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("P1P3E2B_ACQUISITION_LOCK.json"))
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    names = ("review-packets.json", "judgments-template.json", "context-rubric.md", "opening-receipt.json")
    if any((args.output_dir / name).exists() for name in names):
        raise SystemExit(f"refusing to overwrite reserve release in {args.output_dir}")

    ledger_path = args.bank_dir / "private-candidate-ledger.jsonl"
    packets, receipt = make_packets(
        json.loads(args.gate_json.read_text(encoding="utf-8")),
        read_jsonl(ledger_path),
        json.loads(args.lock.read_text(encoding="utf-8")),
    )
    packet_path = args.output_dir / "review-packets.json"
    template_path = args.output_dir / "judgments-template.json"
    rubric_path = args.output_dir / "context-rubric.md"
    receipt_path = args.output_dir / "opening-receipt.json"
    template = [{"packet_id": row["packet_id"], "judgment": None} for row in packets]
    packet_path.write_text(json.dumps(packets, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    template_path.write_text(json.dumps(template, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
    frozen_rubric = Path(__file__).with_name("context-rubric.md")
    rubric_path.write_bytes(frozen_rubric.read_bytes())
    receipt["inputs"] = {
        "support_gate_sha256": sha256(args.gate_json),
        "private_ledger_sha256": sha256(ledger_path),
        "acquisition_lock_sha256": sha256(args.lock),
        "reserve_opener_sha256": sha256(Path(__file__).resolve()),
        "frozen_rubric_sha256": sha256(Path(__file__).with_name("context-rubric.md")),
    }
    receipt["outputs"] = {
        "review_packets_sha256": sha256(packet_path),
        "judgments_template_sha256": sha256(template_path),
        "rubric_sha256": sha256(rubric_path),
    }
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({"status": receipt["status"], "opened_relations": receipt["opened_relations"], "count": len(packets), "outputs": [str(packet_path), str(template_path), str(receipt_path)]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
