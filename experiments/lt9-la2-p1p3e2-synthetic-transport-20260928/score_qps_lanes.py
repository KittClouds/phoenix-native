"""Apply a frozen weighted gate to real QPS search outputs and score E2 sanity lanes."""

from __future__ import annotations

import csv
import hashlib
import json
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

from lexical_gate import context, decide, features, score


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def judged_targets(root: Path) -> dict[tuple, str]:
    ledger = json.loads((root / "private-ledger.json").read_text(encoding="utf-8"))
    judgments = json.loads((root / "judgments-user-pass1.json").read_text(encoding="utf-8"))
    labels = {row["packet_id"]: row["judgment"] for row in judgments}
    return {(row["dataset"], row["query_id"], row["document_ordinal"], row["candidate_id"]):
            labels[row["packet_id"]] for row in ledger}


def qrels_for(rows: list[dict], cohort: dict) -> dict[tuple[str, str], set[str]]:
    roots = {entry["name"]: Path(entry["root"]) for entry in cohort["datasets"]}
    needed: dict[str, set[str]] = defaultdict(set)
    splits: dict[str, set[str]] = defaultdict(set)
    for row in rows:
        candidate = row["candidate"]
        needed[candidate["dataset"]].add(candidate["query_id"])
        splits[candidate["dataset"]].update(candidate["qrels_splits"])
    truth: dict[tuple[str, str], set[str]] = defaultdict(set)
    for dataset in needed:
        for split in sorted(splits[dataset]):
            path = roots[dataset] / "qrels" / f"{split}.tsv"
            with path.open("r", encoding="utf-8", newline="") as stream:
                for record in csv.DictReader(stream, delimiter="\t"):
                    if record["query-id"] in needed[dataset] and int(record["score"]) > 0:
                        truth[(dataset, record["query-id"])].add(record["corpus-id"])
    return truth


def merge(row: dict, admitted: set[int]) -> list[dict]:
    base = {hit["ordinal"]: hit for hit in row["baseline_hits"]}
    expanded = {hit["ordinal"]: hit for hit in row["expanded_hits"]}
    merged = list(base.values())
    merged.extend(expanded[ordinal] for ordinal in admitted if ordinal not in base and ordinal in expanded)
    merged.sort(key=lambda hit: (-hit["score"], hit["ordinal"]))
    return merged[:100]


def main() -> None:
    if len(sys.argv) not in (6, 7, 8):
        raise SystemExit("usage: score_qps_lanes.py MODEL QPS_OUTPUT_JSONL E2_REVIEW_DIR COHORT_JSON NEW_OUTPUT_DIR [LFM_DECISIONS_JSON] [DIRECT_ENDPOINT_DECISIONS_JSON]")
    model_path, searches_path, review_dir, cohort_path, out = (
        Path(argument) for argument in sys.argv[1:6]
    )
    out.mkdir(parents=True, exist_ok=False)
    model = json.loads(model_path.read_text(encoding="utf-8"))
    rows = [json.loads(line) for line in searches_path.read_text(encoding="utf-8").splitlines()]
    lfm_path = Path(sys.argv[6]) if len(sys.argv) >= 7 else None
    lfm_decisions = json.loads(lfm_path.read_text(encoding="utf-8")) if lfm_path else None
    direct_path = Path(sys.argv[7]) if len(sys.argv) == 8 else None
    direct_decisions = json.loads(direct_path.read_text(encoding="utf-8")) if direct_path else None
    labels = judged_targets(review_dir)
    qrels = qrels_for(rows, json.loads(cohort_path.read_text(encoding="utf-8")))
    details = []
    lane_names = ["L0_BASELINE", "L1_UNCONDITIONAL", "L2_WEIGHTED_GATE",
                  "L3_LABELED_TARGET_ORACLE"]
    if lfm_decisions is not None:
        lane_names.append("L4_LFM230_READOUT")
    if direct_decisions is not None:
        lane_names.append("L5_DIRECT_ENDPOINT")
    metrics = {lane: Counter() for lane in lane_names}
    prediction_times = []
    for row_index, row in enumerate(rows):
        candidate = row["candidate"]
        key = (candidate["dataset"], candidate["query_id"],
               candidate["document_ordinal"], candidate["candidate_id"])
        if key not in labels:
            raise ValueError(f"missing reviewed E2 target: {key}")
        label = labels[key]
        source, target = candidate["direction"].split("->")
        query = context(row["query_masked_context"], source, target)
        base_ids = {hit["ordinal"] for hit in row["baseline_hits"]}
        new_hits = row["new_context_hits"]
        decisions = {}
        scores = {}
        for hit in new_hits:
            start = time.perf_counter_ns()
            document = context(hit["masked_context"], source, target)
            vector = features(query, document, model["idf"])
            decisions[hit["ordinal"]] = decide(model, vector)
            scores[hit["ordinal"]] = score(model, vector)
            prediction_times.append((time.perf_counter_ns() - start) / 1000.0)
        target_id = candidate["document_ordinal"]
        relevant = qrels[(candidate["dataset"], candidate["query_id"])]
        doc_ids = {int(ordinal): identity for ordinal, identity in row["document_ids"].items()}
        lanes = {
            "L0_BASELINE": set(),
            "L1_UNCONDITIONAL": {hit["ordinal"] for hit in new_hits},
            "L2_WEIGHTED_GATE": {ordinal for ordinal, decision in decisions.items() if decision == "ALLOW"},
            "L3_LABELED_TARGET_ORACLE": {target_id} if label == "SAME" and target_id in decisions else set(),
        }
        if lfm_decisions is not None:
            per_row = lfm_decisions.get(str(row_index), {})
            lanes["L4_LFM230_READOUT"] = {
                int(ordinal) for ordinal, decision in per_row.items() if decision == "ALLOW"
            }
        if direct_decisions is not None:
            per_row = direct_decisions.get(str(row_index), {})
            lanes["L5_DIRECT_ENDPOINT"] = {
                int(ordinal) for ordinal, decision in per_row.items() if decision == "ALLOW"
            }
        result = {"key": key, "label": label, "qrels_grade": candidate["qrels_grade"],
                  "reported_prior_baseline_rank": row["prior_baseline_rank"],
                  "target_decision": decisions.get(target_id, "NOT_IN_EXPANSION"),
                  "target_gate_scores": scores.get(target_id),
                  "qps_baseline_us": row["baseline_microseconds"],
                  "qps_expanded_us": row["expanded_microseconds"], "lanes": {}}
        for lane, admitted in lanes.items():
            ranked = merge(row, admitted)
            ordinals = [hit["ordinal"] for hit in ranked]
            rank = ordinals.index(target_id) + 1 if target_id in ordinals else None
            relevant_top10 = sum(doc_ids.get(ordinal) in relevant for ordinal in ordinals[:10])
            relevant_top100 = sum(doc_ids.get(ordinal) in relevant for ordinal in ordinals)
            newly_admitted = [ordinal for ordinal in ordinals if ordinal not in base_ids]
            newly_relevant = sum(doc_ids.get(ordinal) in relevant for ordinal in newly_admitted)
            metrics[lane]["probes"] += 1
            metrics[lane]["target_hits_top100"] += rank is not None
            metrics[lane]["same_target_hits_top100"] += label == "SAME" and rank is not None
            metrics[lane]["different_target_hits_top100"] += label == "DIFFERENT" and rank is not None
            metrics[lane]["relevant_top10_sum"] += relevant_top10
            metrics[lane]["relevant_top100_sum"] += relevant_top100
            metrics[lane]["newly_admitted_qrels_positive"] += newly_relevant
            metrics[lane]["newly_admitted_qrels_unjudged"] += len(newly_admitted) - newly_relevant
            metrics[lane]["transport_contexts_admitted"] += len(admitted)
            result["lanes"][lane] = {
                "target_rank": rank, "relevant_top10": relevant_top10,
                "relevant_top100": relevant_top100, "newly_admitted": len(newly_admitted),
                "newly_relevant": newly_relevant,
            }
        details.append(result)
    detail_path = out / "qps-lane-details.json"
    detail_path.write_text(json.dumps(details, indent=2), encoding="utf-8")
    latencies = sorted(prediction_times)
    receipt = {
        "schema": "phoenix.lexical.weighted-gate-qps-lanes/v1",
        "status": "E2_APPLICATION_SANITY_NOT_AUTHORITY_OR_EXTERNAL_QUALIFICATION",
        "model_sha256": digest(model_path), "qps_searches_sha256": digest(searches_path),
        "review_labels_sha256": digest(review_dir / "judgments-user-pass1.json"),
        "lfm_decisions_sha256": digest(lfm_path) if lfm_path else None,
        "direct_endpoint_decisions_sha256": digest(direct_path) if direct_path else None,
        "probe_rows": len(rows), "reviewed_labels": dict(Counter(row["label"] for row in details)),
        "lanes": {lane: dict(value) for lane, value in metrics.items()},
        "latency": {
            "qps_baseline_us_sum": sum(row["baseline_microseconds"] for row in rows),
            "qps_expanded_us_sum": sum(row["expanded_microseconds"] for row in rows),
            "gate_python_us_p50": latencies[len(latencies) // 2] if latencies else None,
            "gate_python_us_p95": latencies[min(len(latencies) - 1, int(len(latencies) * .95))] if latencies else None,
            "gate_evaluations": len(latencies),
        },
        "limits": [
            "Only 17 discovery-exposed, qrels-positive target probes; no external generalization claim",
            "L3 oracle allows only the one reviewed target per probe, not unreviewed candidates",
            "Qrels-absent newly admitted documents are unjudged, not proven irrelevant",
            "Python gate latency is engineering instrumentation, not a Rust serving benchmark",
        ],
    }
    (out / "qps-lane-receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
