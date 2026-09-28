#!/usr/bin/env python3
"""Build the frozen, label-blind P1P3E2-C extension once."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


HERE = Path(__file__).resolve().parent
NS = "P1P3E2C_EXTENSION"
US = "\x1f"
DEFAULT_OUT = Path(r"D:\phoenix-evals\lt9-la2-p1p3e2c-extension-20260928\p1p3e2c-extension-v1")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest(seed: int, *parts: str) -> str:
    return sha256(US.join((str(seed), NS, *parts)).encode("utf-8"))


def file_matches(path: Path, expected: dict[str, Any]) -> bytes:
    raw = path.read_bytes()
    if len(raw) != expected["bytes"] or sha256(raw) != expected["sha256"]:
        raise ValueError(f"locked artifact changed: {path}")
    return raw


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected JSON object: {path}")
    return value


def load_builder_module(path: Path, relations: list[tuple[str, str]]):
    spec = importlib.util.spec_from_file_location("p1p3e2b_parent_builder", path)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load hash-locked E2B extraction primitives")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    module.RELATIONS = relations
    module.digest_key = digest
    return module


def load_excluded(lock: dict[str, Any], parent_builder: Any) -> set[tuple[str, str, str]]:
    ext = lock["extension"]["identity_exclusion_inputs"]
    projection_meta = ext["inherited_projection"]
    projection_raw = file_matches(Path(projection_meta["path"]), projection_meta)
    projection = json.loads(projection_raw)
    excluded = {
        parent_builder.canonical_identity(row["dataset"], row["kind"], str(row["id"]))
        for row in projection
    }
    if len(excluded) != projection_meta["unique_identity_count"]:
        raise ValueError("inherited identity projection count mismatch")

    ledger_meta = ext["e2b_primary_and_all_reserve_candidate_rows"]
    ledger_path = Path(ledger_meta["path"])
    ledger_raw = file_matches(ledger_path, ledger_meta)
    count = 0
    for raw_line in ledger_raw.splitlines():
        if not raw_line:
            continue
        row = json.loads(raw_line)
        # The inherited ledger is consulted only for the three locked identity fields.
        dataset = str(row["dataset"])
        query_id = str(row["query_id"])
        document_id = str(row["document_id"])
        excluded.add(parent_builder.canonical_identity(dataset, "query", query_id))
        excluded.add(parent_builder.canonical_identity(dataset, "document", document_id))
        count += 1
    if count != ledger_meta["candidate_rows"]:
        raise ValueError("E2B identity ledger row count mismatch")
    return excluded


def make_extension_candidate(builder: Any, source: str, target: str, query: Any, document: Any, seed: int):
    return builder.make_candidate(source, target, query, document, seed)


def unique_candidates(builder: Any, queries: dict, docs: dict, relation: str, source: str,
                      target: str, datasets: list[str], seed: int) -> tuple[list[Any], dict[str, int]]:
    unique: dict[tuple[str, str], Any] = {}
    counts: Counter[str] = Counter()
    for dataset in datasets:
        query_rows = queries.get((dataset, relation), [])
        document_rows = docs.get((dataset, relation), [])
        counts[f"{dataset}.query_occurrences"] = len(query_rows)
        counts[f"{dataset}.document_occurrences"] = len(document_rows)
        for query in query_rows:
            sampled = __import__("heapq").nsmallest(
                96,
                document_rows,
                key=lambda row: (digest(seed, "PAIR", dataset, relation, query.identity, row.identity), row.identity),
            )
            for document in sampled:
                candidate = make_extension_candidate(builder, source, target, query, document, seed)
                key = (builder.nfc(candidate.query_context), builder.nfc(candidate.document_context))
                old = unique.get(key)
                candidate_rank = (digest(seed, "PAIR", dataset, relation, query.identity, document.identity), candidate.pair_identity)
                if old is None:
                    unique[key] = candidate
                else:
                    old_rank = (digest(seed, "PAIR", old.dataset, relation, old.query_id, old.document_id), old.pair_identity)
                    if candidate_rank < old_rank:
                        unique[key] = candidate
        counts[f"{dataset}.pairs_considered"] = len(query_rows) * min(96, len(document_rows))
    return list(unique.values()), dict(counts)


def lane_sort(builder: Any, candidate: Any, lane: str, seed: int) -> tuple[Any, ...]:
    tie = digest(seed, "PAIR", candidate.dataset, candidate.relation,
                 candidate.query_id, candidate.document_id)
    if lane == "SEMANTIC_NEAR":
        return (-candidate.jaccard, -candidate.shared, tie, candidate.pair_identity)
    if lane == "SENSE_CONTRAST":
        return (candidate.jaccard, candidate.shared, tie, candidate.pair_identity)
    return (min(candidate.query_visible, candidate.document_visible),
            -abs(candidate.query_visible - candidate.document_visible),
            candidate.query_visible + candidate.document_visible, tie,
            candidate.pair_identity)


def allocate_relation(candidates: list[Any], relation: str, lock: dict[str, Any],
                      identity_split: dict[tuple[str, str, str], str]) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    ext = lock["extension"]
    order = lock["extension"]["acquisition_lanes"]["order"]
    seed = lock["extension"]["ordering"]["seed"]
    dev_lane = dict(zip(order, ext["acquisition_lanes"]["dev_counts_per_relation"]))
    test_lane = dict(zip(order, ext["acquisition_lanes"]["test_counts_per_relation"]))
    caps = {"DEV-EXT": ext["identity_firewall"]["max_per_corpus_per_relation_dev"],
            "TEST-EXT": ext["identity_firewall"]["max_per_corpus_per_relation_test"]}
    used_contexts: set[tuple[str, str]] = set()
    corpus_counts: dict[str, Counter[str]] = {name: Counter() for name in caps}
    counts: Counter[str] = Counter()
    selected: list[dict[str, Any]] = []
    lane_pools = {lane: sorted((c for c in candidates if c.relation == relation),
                               key=lambda c: lane_sort(None, c, lane, seed)) for lane in order}
    id_fn = lambda dataset, kind, identity: (dataset.casefold(), kind, str(identity))

    for lane in order:
        for split_name, need in (("DEV-EXT", dev_lane[lane]), ("TEST-EXT", test_lane[lane])):
            slot = f"{relation}|{lane}|{split_name}"
            for candidate in lane_pools[lane]:
                if counts[slot] >= need:
                    break
                context_key = (candidate.query_context, candidate.document_context)
                if context_key in used_contexts:
                    continue
                qkey = id_fn(candidate.dataset, "query", candidate.query_id)
                dkey = id_fn(candidate.dataset, "document", candidate.document_id)
                if any(identity_split.get(key, split_name) != split_name for key in (qkey, dkey)):
                    continue
                if corpus_counts[split_name][candidate.dataset] >= caps[split_name]:
                    continue
                identity_split[qkey] = split_name
                identity_split[dkey] = split_name
                used_contexts.add(context_key)
                corpus_counts[split_name][candidate.dataset] += 1
                counts[slot] += 1
                selected.append({
                    "packet_id": "",
                    "base_group_id": candidate.base_group_id,
                    "relation": candidate.relation,
                    "source": candidate.source,
                    "target": candidate.target,
                    "dataset": candidate.dataset,
                    "query_id": candidate.query_id,
                    "document_id": candidate.document_id,
                    "lane": lane,
                    "split": split_name,
                    "query_context": candidate.query_context,
                    "document_context": candidate.document_context,
                    "query_non_candidate_tokens": candidate.query_visible,
                    "document_non_candidate_tokens": candidate.document_visible,
                    "content_jaccard": round(candidate.jaccard, 8),
                    "shared_content_tokens": candidate.shared,
                    "query_field_source": candidate.query_field,
                    "document_field_source": candidate.document_field,
                    "label": None,
                })
    missing = {}
    for lane in order:
        for split_name, need in (("DEV-EXT", dev_lane[lane]), ("TEST-EXT", test_lane[lane])):
            slot = f"{relation}|{lane}|{split_name}"
            if counts[slot] < need:
                missing[slot] = {"required": need, "selected": counts[slot]}
    return selected, {"counts": dict(sorted(counts.items())), "corpus_counts": {
        split: dict(sorted(values.items())) for split, values in corpus_counts.items()}, "underfill": missing}


def assign_packet_ids(rows: list[dict[str, Any]], seed: int) -> list[dict[str, Any]]:
    for row in rows:
        row["packet_id"] = "e2c-" + digest(seed, "PACKET", row["base_group_id"])[:16]
    rows.sort(key=lambda row: (digest(seed, "DISPLAY", row["packet_id"]), row["packet_id"]))
    return rows


def write_json(path: Path, value: Any) -> None:
    with path.open("w", encoding="utf-8", newline="\n") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write("\n")


def validate_execution_inputs(exec_path: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], Any, set[tuple[str, str, str]]]:
    exec_lock = load_json(exec_path)
    authorized_commit = exec_lock["authorized_by_commit"]
    subprocess.run(["git", "merge-base", "--is-ancestor", authorized_commit, "HEAD"],
                   cwd=HERE.parents[1], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    builder_meta = exec_lock["builder"]
    file_matches(HERE / builder_meta["path"], builder_meta)
    c_meta = exec_lock["p1p3e2c_lock"]
    c_path = HERE / c_meta["path"]
    c_raw = file_matches(c_path, c_meta)
    c_lock = json.loads(c_raw)
    amendment_meta = exec_lock["amendment_lock"]
    amendment_path = HERE / amendment_meta["path"]
    amendment_raw = file_matches(amendment_path, amendment_meta)
    amendment = json.loads(amendment_raw)
    amendment_file = amendment["amendment_file"]
    amendment_text = (HERE / amendment_file["path"]).read_bytes()
    if sha256(amendment_text) != amendment_file["sha256"]:
        raise ValueError("amendment text does not match its lock")
    if amendment["status"] != "FROZEN_PRE_ACQUISITION" or amendment["acquisition_contract_unchanged"] is not True:
        raise ValueError("amendment is not a frozen pre-acquisition lock")
    protocol_meta = exec_lock["p1p3e2c_protocol"]
    file_matches(HERE / protocol_meta["path"], protocol_meta)
    b_lock_meta = exec_lock["e2b_lock"]
    file_matches(HERE / b_lock_meta["path"], b_lock_meta)
    b_spec_meta = exec_lock["e2b_spec"]
    file_matches(HERE / b_spec_meta["path"], b_spec_meta)
    b_builder_meta = exec_lock["e2b_builder"]
    file_matches(HERE / b_builder_meta["path"], b_builder_meta)
    module = load_builder_module(HERE / b_builder_meta["path"],
                                 [tuple(item.split("->", 1)) for item in c_lock["extension"]["candidate_generation"]["relations"]])
    excluded = load_excluded(c_lock, module)
    return exec_lock, c_lock, amendment, module, excluded


def create_artifacts(exec_lock_path: Path, out: Path) -> dict[str, Any]:
    if out.exists() and any(out.iterdir()):
        raise FileExistsError(f"refusing to overwrite nonempty acquisition directory: {out}")
    exec_lock, lock, amendment, builder, excluded = validate_execution_inputs(exec_lock_path)
    ext = lock["extension"]
    # collect_occurrences reads only the locked JSONL _id/text/title fields; it does not load qrels or labels.
    parent_lock = {"seed": ext["ordering"]["seed"], "source_cohort": ext["source_cohort"], "_excluded": excluded}
    queries, documents, read_counts = builder.collect_occurrences(parent_lock)
    datasets = [row["name"] for row in ext["source_cohort"]]
    global_identity_split: dict[tuple[str, str, str], str] = {}
    rows: list[dict[str, Any]] = []
    relation_reports: dict[str, Any] = {}
    generation_counts: dict[str, Any] = {}
    for relation in ext["candidate_generation"]["relations"]:
        source, target = relation.split("->", 1)
        candidates, generation = unique_candidates(builder, queries, documents, relation, source, target, datasets,
                                                   ext["ordering"]["seed"])
        selected, report = allocate_relation(candidates, relation, lock, global_identity_split)
        rows.extend(selected)
        relation_reports[relation] = report
        generation_counts[relation] = {"unique_context_pairs": len(candidates), **generation}

    underfill = {relation: report["underfill"] for relation, report in relation_reports.items() if report["underfill"]}
    all_filled = not underfill and len(rows) == ext["candidate_count"]["total"]
    rows = assign_packet_ids(rows, ext["ordering"]["seed"])
    out.mkdir(parents=True, exist_ok=True)
    private_path = out / "private-candidate-ledger.jsonl"
    with private_path.open("w", encoding="utf-8", newline="\n") as stream:
        for row in rows:
            stream.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")

    outputs: list[Path] = [private_path]
    if all_filled:
        review_rows = [{"packet_id": row["packet_id"], "lexical_relation": row["relation"],
                        "query_context": row["query_context"], "document_context": row["document_context"]}
                       for row in rows]
        review_path = out / "review-packets.json"
        write_json(review_path, review_rows)
        template_path = out / "judgments-template.json"
        write_json(template_path, [{"packet_id": row["packet_id"], "judgment": None} for row in rows])
        outputs.extend((review_path, template_path))

    split_identities: dict[str, set[tuple[str, str, str]]] = defaultdict(set)
    packet_counts: Counter[str] = Counter()
    relation_counts: Counter[str] = Counter()
    for row in rows:
        split = row["split"]
        packet_counts[split] += 1
        relation_counts[f"{row['relation']}|{split}"] += 1
        split_identities[split].add((row["dataset"].casefold(), "query", row["query_id"]))
        split_identities[split].add((row["dataset"].casefold(), "document", row["document_id"]))
    identity_overlap = len(split_identities["DEV-EXT"] & split_identities["TEST-EXT"])

    report: dict[str, Any] = {
        "schema": "phoenix.lt9.la2.p1p3e2c-acquisition-receipt.v1",
        "status": "ACQUISITION_COMPLETE_LABELS_UNOPENED" if all_filled else "UNDERPOWERED_LABEL_BLIND_REVIEW_NOT_RELEASED",
        "execution_lock_sha256": sha256(exec_lock_path.read_bytes()),
        "p1p3e2c_lock_sha256": sha256((HERE / exec_lock["p1p3e2c_lock"]["path"]).read_bytes()),
        "amendment_lock_sha256": sha256((HERE / exec_lock["amendment_lock"]["path"]).read_bytes()),
        "source_read_counts": read_counts,
        "generation_counts": generation_counts,
        "relation_selection": relation_reports,
        "selected_total": len(rows),
        "selected_by_split": dict(sorted(packet_counts.items())),
        "selected_by_relation_split": dict(sorted(relation_counts.items())),
        "identity_overlap_dev_test": identity_overlap,
        "underfill": underfill,
        "review_packets_released": all_filled,
        "labels_joined": False,
        "qrels_or_ranks_opened": False,
        "feature_join_or_model_contact": False,
        "fitting_or_retrieval_run": False,
        "authority_or_serving_changed": False,
    }
    for path in outputs:
        report[path.name] = {"bytes": path.stat().st_size, "sha256": sha256(path.read_bytes())}
    receipt_path = out / "acquisition-receipt.json"
    write_json(receipt_path, report)
    return report


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--execution-lock", type=Path, default=HERE / "P1P3E2C_EXTENSION_EXECUTION_LOCK.json")
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    args = parser.parse_args()
    try:
        receipt = create_artifacts(args.execution_lock, args.out)
    except Exception as exc:
        print(f"P1P3E2C extension acquisition failed closed: {exc}", file=sys.stderr)
        return 2
    print(json.dumps({"status": receipt["status"], "selected_total": receipt["selected_total"],
                      "selected_by_split": receipt["selected_by_split"], "underfilled_relations": len(receipt["underfill"]),
                      "review_packets_released": receipt["review_packets_released"],
                      "receipt": str(args.out / "acquisition-receipt.json")}, indent=2))
    return 0 if receipt["status"] == "ACQUISITION_COMPLETE_LABELS_UNOPENED" else 1


if __name__ == "__main__":
    raise SystemExit(main())
