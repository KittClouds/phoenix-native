"""Create blind context-review packets from the frozen P1P3E2 rank output."""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import sys
from pathlib import Path


TOKEN_RE = re.compile(r"\w+", re.UNICODE)
MASK_RE_CACHE: dict[str, re.Pattern[str]] = {}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def context_windows(text: str, term: str, radius: int = 12) -> list[str]:
    tokens = list(TOKEN_RE.finditer(text))
    positions = [i for i, match in enumerate(tokens) if match.group().casefold() == term.casefold()]
    windows: list[str] = []
    for index in positions[:3]:
        start = max(0, index - radius)
        end = min(len(tokens), index + radius + 1)
        windows.append(text[tokens[start].start() : tokens[end - 1].end()])
    return windows


def mask_term(text: str, term: str, marker: str) -> str:
    pattern = MASK_RE_CACHE.get(term)
    if pattern is None:
        pattern = re.compile(rf"(?<!\w){re.escape(term)}(?!\w)", re.IGNORECASE | re.UNICODE)
        MASK_RE_CACHE[term] = pattern
    return pattern.sub(marker, text)


def packet_id(row: dict) -> str:
    candidate = row["candidate"]
    identity = "|".join(
        (
            candidate["dataset"],
            candidate["candidate_id"],
            candidate["query_id"],
            candidate["document_id"],
        )
    )
    return "e2-" + hashlib.sha256(identity.encode("utf-8")).hexdigest()[:16]


def endpoint_forms(candidate: dict) -> tuple[str, str]:
    source, target = candidate["direction"].split("->", 1)
    if sorted((source, target)) != sorted(candidate["lexical_pair"]):
        raise ValueError(f"direction and unordered pair disagree: {candidate['candidate_id']}")
    return source, target


def document_target_windows(ranked_rows: list[dict], cohort: dict, rank_receipt: dict) -> tuple[dict, dict]:
    corpus_roots = {row["name"]: Path(row["root"]) for row in cohort["datasets"]}
    receipt_rows = {row["name"]: row for row in rank_receipt["datasets"]}
    requested: dict[str, dict[int, set[str]]] = {}
    for row in ranked_rows:
        candidate = row["candidate"]
        _, target = endpoint_forms(candidate)
        requested.setdefault(candidate["dataset"], {}).setdefault(
            int(candidate["document_ordinal"]), set()
        ).add(target)

    extracted: dict[tuple[str, int, str], list[str]] = {}
    observed_hashes: dict[str, str] = {}
    for dataset, ordinal_targets in requested.items():
        root = corpus_roots.get(dataset)
        expected = receipt_rows.get(dataset)
        if root is None or expected is None:
            raise ValueError(f"missing frozen corpus/receipt entry for {dataset}")
        corpus_path = root / "corpus.jsonl"
        digest = hashlib.sha256()
        documents = 0
        pending = set(ordinal_targets)
        with corpus_path.open("rb") as stream:
            for ordinal, line in enumerate(stream):
                digest.update(line)
                if ordinal in pending:
                    document = json.loads(line)
                    expected_ids = {
                        row["candidate"]["document_id"]
                        for row in ranked_rows
                        if row["candidate"]["dataset"] == dataset
                        and int(row["candidate"]["document_ordinal"]) == ordinal
                    }
                    if str(document.get("_id")) not in expected_ids:
                        raise ValueError(f"document ordinal/id mismatch: {dataset}:{ordinal}")
                    text = " ".join((document.get("title") or "", document.get("text") or ""))
                    for target in ordinal_targets[ordinal]:
                        windows = context_windows(text, target)
                        if not windows:
                            raise ValueError(f"target {target!r} missing at {dataset}:{ordinal}")
                        extracted[(dataset, ordinal, target)] = windows
                    pending.remove(ordinal)
                documents = ordinal + 1
        observed = digest.hexdigest()
        if observed != expected["corpus_sha256"]:
            raise ValueError(f"corpus changed since QPS ranking: {dataset}")
        if documents != expected["corpus_documents"]:
            raise ValueError(f"document count changed since QPS ranking: {dataset}")
        if pending:
            raise ValueError(f"candidate document ordinals missing from {dataset}: {sorted(pending)}")
        observed_hashes[dataset] = observed
    return extracted, observed_hashes


def main() -> int:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: make_context_packets.py <ranked-opportunities.jsonl> "
            "<qps-baseline-receipt.json> <cohort.json> <new-output-dir>"
        )

    ranked_path = Path(sys.argv[1])
    rank_receipt_path = Path(sys.argv[2])
    cohort_path = Path(sys.argv[3])
    output_dir = Path(sys.argv[4])
    if output_dir.exists():
        raise SystemExit(f"refusing existing output directory: {output_dir}")

    ranked_rows = [
        json.loads(line)
        for line in ranked_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    rank_receipt = json.loads(rank_receipt_path.read_text(encoding="utf-8"))
    cohort = json.loads(cohort_path.read_text(encoding="utf-8"))
    target_windows, corpus_hashes = document_target_windows(ranked_rows, cohort, rank_receipt)
    packets: list[dict] = []
    ledger: list[dict] = []
    opportunities: list[dict] = []

    for row in ranked_rows:
        candidate = row["candidate"]
        source, target = endpoint_forms(candidate)
        left = context_windows(candidate["query_text"], source)
        right = target_windows[
            (candidate["dataset"], int(candidate["document_ordinal"]), target)
        ]
        if not left:
            raise ValueError(f"source {source!r} missing from query {candidate['query_id']}")
        status = row["baseline_status"]

        visible = {
            "packet_id": packet_id(row),
            "lexical_relation": [source, target],
            "orientation": "query_source_to_document_counterpart",
            "query_contexts": [
                mask_term(mask_term(value, source, "[SOURCE]"), target, "[TARGET]")
                for value in left
            ],
            "document_contexts": [
                mask_term(mask_term(value, source, "[SOURCE]"), target, "[TARGET]")
                for value in right
            ],
            "judgment": None,
        }
        packets.append(visible)
        ledger.append(
            {
                "packet_id": visible["packet_id"],
                "dataset": candidate["dataset"],
                "candidate_id": candidate["candidate_id"],
                "query_id": candidate["query_id"],
                "document_id": candidate["document_id"],
                "document_ordinal": candidate["document_ordinal"],
                "qrels_splits": candidate["qrels_splits"],
                "qrels_grade": candidate["qrels_grade"],
                "partition": candidate["partition"],
                "baseline_rank": row["baseline_rank"],
                "baseline_status": status,
                "query_source_context_count": len(left),
                "document_target_context_count": len(right),
            }
        )
        if status in ("MISSED_TOP100", "UNDERRANKED_11_100"):
            opportunities.append(row)

    packets.sort(key=lambda item: item["packet_id"])
    ledger.sort(key=lambda item: item["packet_id"])
    if len({packet["packet_id"] for packet in packets}) != len(packets):
        raise ValueError("duplicate packet identity in ranked input")
    output_dir.mkdir(parents=True)
    (output_dir / "review-packets.json").write_text(
        json.dumps(packets, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (output_dir / "judgments-template.json").write_text(
        json.dumps(
            [{"packet_id": packet["packet_id"], "judgment": None} for packet in packets],
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    (output_dir / "private-ledger.json").write_text(
        json.dumps(ledger, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (output_dir / "transport-opportunity-bank.jsonl").write_text(
        "".join(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n" for row in opportunities),
        encoding="utf-8",
    )
    shutil.copyfile(Path(__file__).with_name("context-rubric.md"), output_dir / "rubric.md")

    files = [
        "rubric.md",
        "review-packets.json",
        "judgments-template.json",
        "private-ledger.json",
        "transport-opportunity-bank.jsonl",
    ]
    receipt = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2-context-bank-acquisition/v1",
        "date": "2026-09-28",
        "status": "BLIND_CONTEXT_PACKETS_PREPARED_NO_LABELS_OR_FIT",
        "ranked_input_sha256": sha256_file(ranked_path),
        "qps_receipt_sha256": sha256_file(rank_receipt_path),
        "cohort_sha256": sha256_file(cohort_path),
        "corpus_sha256": corpus_hashes,
        "packet_count": len(packets),
        "opportunity_count": len(opportunities),
        "packet_ids_unchanged_by_template": True,
        "visible_packets_exclude_qrels_and_rank_metadata": True,
        "feature_join_or_model_fit": False,
        "outputs": [
            {"path": name, "sha256": sha256_file(output_dir / name), "bytes": (output_dir / name).stat().st_size}
            for name in files
        ],
    }
    (output_dir / "acquisition-receipt.json").write_text(
        json.dumps(receipt, indent=2) + "\n", encoding="utf-8"
    )
    print(json.dumps(receipt, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
