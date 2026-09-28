#!/usr/bin/env python3
"""Join sealed FIT-only context labels to retrieval metadata; no model fitting."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


OPPORTUNITY = {"MISSED_TOP100", "UNDERRANKED_11_100"}
TOP10 = "ALREADY_TOP10"
SPARSE_BINS = ("0-2", "3-5", "6+")
TOKEN_RE = re.compile(r"[A-Za-z0-9]+")
PLACEHOLDERS = {"SOURCE", "TARGET"}


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def sparse_bin(smaller_side: int) -> str:
    if smaller_side <= 2:
        return "0-2"
    if smaller_side <= 5:
        return "3-5"
    return "6+"


def side_count(contexts: list[str]) -> int:
    words = (token.upper() for text in contexts for token in TOKEN_RE.findall(text))
    return sum(token not in PLACEHOLDERS for token in words)


def context_identity(packet: dict[str, Any]) -> str:
    payload = {
        "lexical_relation": packet["lexical_relation"],
        "orientation": packet["orientation"],
        "query_contexts": packet["query_contexts"],
        "document_contexts": packet["document_contexts"],
    }
    return hashlib.sha256(canonical_bytes(payload)).hexdigest()


def load_source(
    source_key: str,
    label_kind: str,
    packet_path: Path,
    ledger_path: Path,
    labels_path: Path,
    provenance_path: Path,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    packets = read_json(packet_path)
    ledger = read_json(ledger_path)
    labels = read_json(labels_path)
    provenance = read_json(provenance_path)
    if not all(isinstance(rows, list) for rows in (packets, ledger, labels)):
        raise ValueError(f"{source_key}: packet, ledger, and label inputs must be arrays")

    packet_ids = [row.get("packet_id") for row in packets]
    ledger_ids = [row.get("packet_id") for row in ledger]
    label_ids = [row.get("packet_id") for row in labels]
    if len(set(packet_ids)) != len(packet_ids) or packet_ids != ledger_ids:
        raise ValueError(f"{source_key}: packet/ledger IDs are duplicate or misordered")
    if len(set(label_ids)) != len(label_ids) or set(label_ids) != set(packet_ids):
        raise ValueError(f"{source_key}: labels do not map one-to-one to the frozen packets")

    ledger_by_id = {row["packet_id"]: row for row in ledger}
    # Keep complete label rows opaque until their packet is confirmed FIT.
    # In particular, do not read a non-FIT label merely to build a lookup.
    labels_by_id = {row["packet_id"]: row for row in labels}
    joined: list[dict[str, Any]] = []
    excluded_non_fit = 0
    for packet in packets:
        packet_id = packet["packet_id"]
        meta = ledger_by_id[packet_id]
        if meta.get("partition") != "FIT":
            excluded_non_fit += 1
            # Deliberately do not read or aggregate labels for non-FIT rows.
            continue
        label = labels_by_id[packet_id].get("judgment")
        if label not in {"SAME", "DIFFERENT", "UNKNOWN"}:
            raise ValueError(f"{source_key}: invalid label {label!r} for {packet_id}")
        relation = packet.get("lexical_relation")
        query = packet.get("query_contexts")
        document = packet.get("document_contexts")
        if not (isinstance(relation, list) and len(relation) == 2 and all(isinstance(x, str) for x in relation)):
            raise ValueError(f"{source_key}: malformed lexical relation for {packet_id}")
        if not (isinstance(query, list) and isinstance(document, list) and all(isinstance(x, str) for x in query + document)):
            raise ValueError(f"{source_key}: malformed context fields for {packet_id}")
        candidate_forms = {value.casefold() for value in relation}
        for text in query + document:
            leaked = candidate_forms.intersection(token.casefold() for token in TOKEN_RE.findall(text))
            if leaked:
                raise ValueError(f"{source_key}: candidate token leaked in {packet_id}: {sorted(leaked)}")
        q_count = side_count(query)
        d_count = side_count(document)
        status = meta.get("baseline_status")
        dataset = str(meta.get("dataset", "unknown"))
        row = {
            "source_key": source_key,
            "label_provenance": label_kind,
            "packet_id": packet_id,
            "relation": "->".join(relation),
            "label": label,
            "dataset": dataset,
            "baseline_status": status,
            "is_opportunity": status in OPPORTUNITY,
            "is_top10_control": status == TOP10,
            "query_noncandidate_ascii_alnum_tokens": q_count,
            "document_non_candidate_ascii_alnum_tokens": d_count,
            "smaller_side_tokens": min(q_count, d_count),
            "sparse_bin": sparse_bin(min(q_count, d_count)),
            "one_sided_sparse_lt3": (q_count < 3) != (d_count < 3),
            "context_identity_sha256": context_identity(packet),
        }
        joined.append(row)

    if len(joined) + excluded_non_fit != len(packets):
        raise AssertionError(f"{source_key}: FIT/non-FIT partition accounting failed")

    if label_kind == "model_origin_proxy":
        expected = provenance["inputs"]
        if expected["model_origin_judgments"]["sha256"] != sha256(labels_path):
            raise ValueError(f"{source_key}: model labels differ from the sealed label receipt")
        if expected["review_packets"]["sha256"] != sha256(packet_path):
            raise ValueError(f"{source_key}: packets differ from the label seal")
    else:
        if provenance.get("judgments_sha256") != sha256(labels_path):
            raise ValueError(f"{source_key}: user label hash differs from import receipt")
        if provenance.get("frozen_packets_sha256") != sha256(packet_path):
            raise ValueError(f"{source_key}: packets differ from label import receipt")

    source_receipt = {
        "source_key": source_key,
        "label_provenance": label_kind,
        "packet_count": len(packets),
        "fit_count": len(joined),
        "excluded_non_fit_count": excluded_non_fit,
        "dataset_counts_fit": dict(sorted(Counter(row["dataset"] for row in joined).items())),
        "files": {
            "packets": {"path": str(packet_path), "sha256": sha256(packet_path)},
            "ledger": {"path": str(ledger_path), "sha256": sha256(ledger_path)},
            "labels": {"path": str(labels_path), "sha256": sha256(labels_path)},
            "label_receipt": {"path": str(provenance_path), "sha256": sha256(provenance_path)},
        },
    }
    return joined, source_receipt


def summarize(rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_relation: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_source: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        by_relation[row["relation"]].append(row)
        by_source[row["source_key"]].append(row)

    relation_summary: dict[str, Any] = {}
    for relation, group in sorted(by_relation.items()):
        labels = Counter(row["label"] for row in group)
        by_corpus = Counter(row["dataset"] for row in group)
        sparse = {
            band: {
                "rows": sum(row["sparse_bin"] == band for row in group),
                "SAME": sum(row["sparse_bin"] == band and row["label"] == "SAME" for row in group),
                "DIFFERENT": sum(row["sparse_bin"] == band and row["label"] == "DIFFERENT" for row in group),
                "UNKNOWN": sum(row["sparse_bin"] == band and row["label"] == "UNKNOWN" for row in group),
            }
            for band in SPARSE_BINS
        }
        status_counts = Counter(row["baseline_status"] for row in group)
        relation_summary[relation] = {
            "reviewed_fit_rows": len(group),
            "unique_context_pairs": len({row["context_identity_sha256"] for row in group}),
            "labels": {label: labels[label] for label in ("SAME", "DIFFERENT", "UNKNOWN")},
            "retrieval_opportunity_rows": sum(row["is_opportunity"] for row in group),
            "opportunity_labels": {
                label: sum(row["is_opportunity"] and row["label"] == label for row in group)
                for label in ("SAME", "DIFFERENT", "UNKNOWN")
            },
            "top10_control_rows": sum(row["is_top10_control"] for row in group),
            "baseline_status_counts": dict(sorted(status_counts.items())),
            "sparse_context_by_smaller_side": sparse,
            "one_sided_sparse_lt3_rows": sum(row["one_sided_sparse_lt3"] for row in group),
            "source_corpora": dict(sorted(by_corpus.items())),
            "review_sources": dict(sorted(Counter(row["source_key"] for row in group).items())),
            "observed_support_state": (
                "INSUFFICIENT_FIT" if labels["SAME"] == 0 else
                "INSUFFICIENT_REFUSE" if labels["DIFFERENT"] == 0 else
                "BOTH_CLASSES_PRESENT" if labels["UNKNOWN"] == 0 else
                "BOTH_CLASSES_AND_NATURAL_UNKNOWN_PRESENT"
            ),
        }

    source_summary: dict[str, Any] = {}
    for source, group in sorted(by_source.items()):
        counts = Counter(row["label"] for row in group)
        source_summary[source] = {
            "fit_rows": len(group),
            "labels": {label: counts[label] for label in ("SAME", "DIFFERENT", "UNKNOWN")},
            "opportunities": sum(row["is_opportunity"] for row in group),
            "opportunity_labels": {
                label: sum(row["is_opportunity"] and row["label"] == label for row in group)
                for label in ("SAME", "DIFFERENT", "UNKNOWN")
            },
            "top10_controls": sum(row["is_top10_control"] for row in group),
            "sparse_context_rows": {band: sum(row["sparse_bin"] == band for row in group) for band in SPARSE_BINS},
            "corpora": dict(sorted(Counter(row["dataset"] for row in group).items())),
        }
    return {"by_relation": relation_summary, "by_source": source_summary}


def deduplicated_support(rows: list[dict[str, Any]]) -> dict[str, Any]:
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[row["context_identity_sha256"]].append(row)
    counts_by_relation: dict[str, Counter[str]] = defaultdict(Counter)
    conflicts: list[dict[str, Any]] = []
    for identity, group in sorted(grouped.items()):
        labels = {row["label"] for row in group}
        relation = group[0]["relation"]
        if len(labels) != 1:
            conflicts.append({
                "context_identity_sha256": identity,
                "relation": relation,
                "sources": sorted({row["source_key"] for row in group}),
                "labels_by_source": {source: sorted({row["label"] for row in group if row["source_key"] == source}) for source in sorted({row["source_key"] for row in group})},
                "disposition": "EXCLUDED_FROM_POOLED_DEFINITE_CLASS_COUNTS_NOT_RELABELLED_UNKNOWN",
            })
            continue
        counts_by_relation[relation][next(iter(labels))] += 1
    relation_rows: dict[str, Any] = {}
    for relation, counts in sorted(counts_by_relation.items()):
        same_ok = counts["SAME"] >= 8
        different_ok = counts["DIFFERENT"] >= 8
        relation_rows[relation] = {
            "labels": {label: counts[label] for label in ("SAME", "DIFFERENT", "UNKNOWN")},
            "candidate_8x8_floor_diagnostic_only": {
                "same_floor_met": same_ok,
                "different_floor_met": different_ok,
                "both_floors_met": same_ok and different_ok,
                "status": "MEETS_UNFROZEN_8x8_REFERENCE" if same_ok and different_ok else "BELOW_UNFROZEN_8x8_REFERENCE",
            },
        }
    return {
        "unique_natural_context_pairs": len(grouped),
        "unique_pairs_with_cross_pass_or_within_pass_label_conflict": len(conflicts),
        "conflicts": conflicts,
        "by_relation": relation_rows,
    }


def markdown_report(result: dict[str, Any]) -> str:
    lines = [
        "# P1P3E2 natural relation support matrix",
        "",
        "**Disposition:** post-label-join discovery report only. No feature fitting, threshold selection, retrieval run, authority update, or serving change.",
        "",
        "The matrix includes only natural FIT-partition rows. Non-FIT rows were excluded before label aggregation; sealed retrieval holdouts remain outside the support totals. Sources retain separate provenance, and duplicate context payloads are deduplicated only for the pooled support view.",
        "",
        "## Relation support by review source",
        "",
        "| Directed relation | Source | FIT/SAME | REFUSE/DIFFERENT | Natural UNKNOWN | Opportunity rows (S/D/U) | Top-10 controls | Sparse 0-2 / 3-5 / 6+ | Corpora |",
        "|---|---|---:|---:|---:|---:|---:|---|---|",
    ]
    for relation, data in result["support"]["by_relation"].items():
        for source, n in sorted(data["review_sources"].items()):
            source_rows = [row for row in result["rows"] if row["relation"] == relation and row["source_key"] == source]
            counts = Counter(row["label"] for row in source_rows)
            opp = [row for row in source_rows if row["is_opportunity"]]
            bands = Counter(row["sparse_bin"] for row in source_rows)
            corpora = Counter(row["dataset"] for row in source_rows)
            lines.append(
                f"| `{relation}` | {source} | {counts['SAME']} | {counts['DIFFERENT']} | {counts['UNKNOWN']} | "
                f"{sum(r['label']=='SAME' for r in opp)}/{sum(r['label']=='DIFFERENT' for r in opp)}/{sum(r['label']=='UNKNOWN' for r in opp)} | "
                f"{sum(r['is_top10_control'] for r in source_rows)} | {bands['0-2']} / {bands['3-5']} / {bands['6+']} | "
                f"{', '.join(f'{k}:{v}' for k,v in sorted(corpora.items()))} |"
            )
    lines += [
        "",
        "## Deduplicated natural-context support",
        "",
        "| Relation | Unique SAME | Unique DIFFERENT | Unique natural UNKNOWN | State | 8/8 reference (unfrozen) |",
        "|---|---:|---:|---:|---|---|",
    ]
    pooled = result["pooled_unique_context_support"]
    for relation, detail in pooled["by_relation"].items():
        counts = detail["labels"]
        state = result["support"]["by_relation"].get(relation, {}).get("observed_support_state", "NO_ROWS")
        candidate = detail.get("candidate_8x8_floor_diagnostic_only", {})
        lines.append(f"| `{relation}` | {counts['SAME']} | {counts['DIFFERENT']} | {counts['UNKNOWN']} | {state} | {candidate.get('status','n/a')} |")
    lines += [
        "",
        f"Exact context payloads across sources: {pooled['unique_natural_context_pairs']} unique pairs; {pooled['unique_pairs_with_cross_pass_or_within_pass_label_conflict']} label-discordant duplicate groups were excluded from pooled definite classes and were not relabeled UNKNOWN.",
        "",
        "## Interpretation and limits",
        "",
        "SAME is the compatibility/FIT label; DIFFERENT is the compatibility REFUSE label; UNKNOWN counts only explicit natural UNKNOWN judgments. Synthetic UNKNOWN variants are not included here. Retrieval opportunities count FIT ledger rows with baseline status MISSED_TOP100 or UNDERRANKED_11_100; top-10 controls are reported separately.",
        "",
        "Sparse bins follow the frozen evaluation protocol: count noncandidate ASCII alphanumeric tokens across each side's visible masked context windows, remove `[SOURCE]`/`[TARGET]`, then bin the smaller side as 0–2, 3–5, or 6+. These are evidence-length strata, not semantic UNKNOWN labels.",
        "",
        "The 8-SAME / 8-DIFFERENT column is a diagnostic against the suggested engineering starting point only. That floor has not been frozen as a policy. Counts from the user-supplied prior review and the Codex model-origin A1 pass remain separately visible; pooled counts deduplicate exact context payloads and exclude discordant duplicate labels.",
        "",
        "No classifier or feature join was fitted from this report. A support state indicates observed class presence only; it does not authorize lexical transport.",
        "",
        "## Inputs and reproducibility",
        "",
    ]
    for source, data in result["sources"].items():
        lines.append(f"- `{source}`: {data['fit_count']}/{data['packet_count']} FIT rows included; {data['excluded_non_fit_count']} non-FIT row excluded; label provenance `{data['label_provenance']}`.")
        for role, meta in data["files"].items():
            lines.append(f"  - {role} SHA-256 `{meta['sha256']}`")
    lines += [
        f"- Support-matrix script SHA-256 `{result['script_sha256']}`",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--a1-dir", type=Path, required=True)
    parser.add_argument("--a1-labels", type=Path, required=True)
    parser.add_argument("--a1-seal", type=Path, required=True)
    parser.add_argument("--prior-dir", type=Path, required=True)
    parser.add_argument("--prior-labels", type=Path, required=True)
    parser.add_argument("--prior-import-receipt", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    a1, a1_receipt = load_source(
        "A1_fiqa_codex_proxy",
        "model_origin_proxy",
        args.a1_dir / "review-packets.json",
        args.a1_dir / "private-ledger.json",
        args.a1_labels,
        args.a1_seal,
    )
    prior, prior_receipt = load_source(
        "prior_natural_context_review",
        "user_supplied_single_pass_identity_unverified",
        args.prior_dir / "review-packets.json",
        args.prior_dir / "private-ledger.json",
        args.prior_labels,
        args.prior_import_receipt,
    )
    rows = prior + a1
    support = summarize(rows)
    pooled = deduplicated_support(rows)
    script_path = Path(__file__).resolve()
    result = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2-natural-relation-support-matrix/v1",
        "status": "DISCOVERY_SUPPORT_MAP_NO_FIT_NO_RETRIEVAL_NO_AUTHORITY_CHANGE",
        "rows": rows,
        "sources": {"A1_fiqa_codex_proxy": a1_receipt, "prior_natural_context_review": prior_receipt},
        "support": support,
        "pooled_unique_context_support": pooled,
        "script_sha256": sha256(script_path),
        "boundaries": {
            "only_natural_fit_partition_rows_counted": True,
            "non_fit_labels_excluded_before_aggregation": True,
            "sealed_retrieval_holdouts_used": False,
            "synthetic_unknown_rows_in_natural_unknown_counts": False,
            "model_fit_or_threshold_selection": False,
            "retrieval_or_serving_run": False,
            "8x8_floor_frozen": False,
        },
    }
    receipt_path = args.output_dir / "relation-support-matrix.json"
    report_path = args.output_dir / "relation-support-matrix.md"
    if receipt_path.exists() or report_path.exists():
        raise SystemExit(f"refusing to overwrite matrix outputs in {args.output_dir}")
    receipt_path.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    report_path.write_text(markdown_report(result), encoding="utf-8", newline="\n")
    print(json.dumps({
        "status": result["status"],
        "source_fit_counts": {key: value["fit_count"] for key, value in result["sources"].items()},
        "pooled_unique_context_pairs": pooled["unique_natural_context_pairs"],
        "duplicate_label_conflicts": pooled["conflicts"],
        "relation_counts": {key: value["labels"] for key, value in support["by_relation"].items()},
        "outputs": [str(report_path), str(receipt_path)],
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
