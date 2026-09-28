#!/usr/bin/env python3
"""Label-blind integrity checks for the frozen P1P3E2-B acquisition artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path
from typing import Any

from build_e2b_natural_bank import (
    LANES,
    PRIMARY_LANE_SPLITS,
    RELATIONS,
    TERM_PATTERNS,
    canonical_identity,
)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    with path.open(encoding="utf-8") as stream:
        return [json.loads(line) for line in stream if line.strip()]


def verify(lock_path: Path, out: Path) -> dict[str, Any]:
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    receipt_path = out / "acquisition-receipt.json"
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if hashlib.sha256(lock_path.read_bytes()).hexdigest() != receipt["lock_sha256"]:
        raise ValueError("receipt lock hash mismatch")
    private_path = out / "private-candidate-ledger.jsonl"
    packet_path = out / "review-packets.json"
    template_path = out / "judgments-template.json"
    rows = read_jsonl(private_path)
    packets = json.loads(packet_path.read_text(encoding="utf-8"))
    template = json.loads(template_path.read_text(encoding="utf-8"))
    for path in (private_path, packet_path, template_path):
        expected = receipt[path.name]
        if path.stat().st_size != expected["bytes"] or sha256(path) != expected["sha256"]:
            raise ValueError(f"artifact hash mismatch: {path.name}")

    primary = [row for row in rows if row["population"] == "PRIMARY"]
    reserves = [row for row in rows if row["population"] == "RESERVE"]
    if len(primary) != 216 or len(reserves) != 108:
        raise ValueError("candidate totals differ from frozen quotas")
    if any(row.get("label") is not None for row in rows):
        raise ValueError("acquisition ledger contains a label")
    if set(row["relation"] for row in rows) != {f"{s}->{t}" for s, t in RELATIONS}:
        raise ValueError("relation roster mismatch")
    if len({row["base_group_id"] for row in rows}) != len(rows):
        raise ValueError("duplicate base group")

    packet_fields = {"packet_id", "lexical_relation", "query_context", "document_context"}
    if len(packets) != len(primary) or any(set(packet) != packet_fields for packet in packets):
        raise ValueError("review packet shape or count mismatch")
    if len(template) != len(primary) or any(set(row) != {"packet_id", "judgment"} or row["judgment"] is not None for row in template):
        raise ValueError("judgment template shape or null state mismatch")
    if {packet["packet_id"] for packet in packets} != {row["packet_id"] for row in primary}:
        raise ValueError("review packets do not match primary candidate IDs")
    if {row["packet_id"] for row in template} != {row["packet_id"] for row in primary}:
        raise ValueError("judgment template packet IDs mismatch")

    split_counts = Counter((r["relation"], r["lane"], r["split"]) for r in primary)
    for source, target in RELATIONS:
        relation = f"{source}->{target}"
        for lane in LANES:
            for split, expected in PRIMARY_LANE_SPLITS[lane].items():
                if split_counts[(relation, lane, split)] != expected:
                    raise ValueError(f"quota mismatch: {relation}/{lane}/{split}")

    primary_by_relation_corpus = Counter((r["relation"], r["dataset"]) for r in primary)
    reserve_by_relation_corpus = Counter((r["relation"], r["dataset"]) for r in reserves)
    if any(count > 12 for count in primary_by_relation_corpus.values()):
        raise ValueError("primary corpus cap exceeded")
    if any(count > 6 for count in reserve_by_relation_corpus.values()):
        raise ValueError("reserve corpus cap exceeded")

    split_ids: dict[str, set[tuple[str, str, str]]] = {name: set() for name in ("TRAIN-NEW", "DEV-NEW", "TEST-NEW")}
    excluded = json.loads(Path(lock["excluded_identity_projection"]["path"]).read_text(encoding="utf-8"))
    excluded_ids = {(row["dataset"].casefold(), row["kind"], str(row["id"])) for row in excluded}
    new_ids: set[tuple[str, str, str]] = set()
    source_terms = {word for pair in RELATIONS for word in pair}
    for row in primary:
        current = split_ids[row["split"]]
        qkey = canonical_identity(row["dataset"], "query", row["query_id"])
        dkey = canonical_identity(row["dataset"], "document", row["document_id"])
        current.update((qkey, dkey))
        new_ids.update((qkey, dkey))
        source, target = row["relation"].split("->")
        for term in (source, target):
            if TERM_PATTERNS[term].search(row["query_context"]) or TERM_PATTERNS[term].search(row["document_context"]):
                raise ValueError(f"candidate term leaked into context: {row['packet_id']}")
    if new_ids & excluded_ids:
        raise ValueError("prior or canary identity entered new bank")
    split_names = list(split_ids)
    overlap = {}
    for i, left in enumerate(split_names):
        for right in split_names[i + 1:]:
            overlap[f"{left}&{right}"] = len(split_ids[left] & split_ids[right])
            if overlap[f"{left}&{right}"]:
                raise ValueError("query/document identity crosses primary splits")

    return {
        "status": "PASS_LABELS_UNOPENED",
        "primary": len(primary),
        "reserve": len(reserves),
        "review_packets": len(packets),
        "labels_present": False,
        "duplicate_base_groups": 0,
        "candidate_term_leaks": 0,
        "excluded_identity_overlap": 0,
        "cross_split_identity_overlap": overlap,
        "split_row_counts": dict(Counter(row["split"] for row in primary)),
        "corpus_cap_max_primary": max(primary_by_relation_corpus.values()),
        "corpus_cap_max_reserve": max(reserve_by_relation_corpus.values()),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("P1P3E2B_ACQUISITION_LOCK.json"))
    parser.add_argument("--out", type=Path, default=Path(r"D:\phoenix-evals\lt9-la2-p1p3e2b-acquisition-20260928\bank-v1"))
    args = parser.parse_args()
    try:
        print(json.dumps(verify(args.lock, args.out), indent=2))
    except Exception as exc:
        print(f"P1P3E2B verification failed: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
