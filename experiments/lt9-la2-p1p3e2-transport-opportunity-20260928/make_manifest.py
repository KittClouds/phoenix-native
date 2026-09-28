"""Seal the P1P3E2 source and external discovery artifacts by content hash."""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(r"D:\phoenix-native-cleanroom-cut4-20260920")
EXPERIMENT = ROOT / "experiments/lt9-la2-p1p3e2-transport-opportunity-20260928"
EVAL = Path(r"D:\phoenix-evals")
RANKER = EVAL / "lt9-la2-p1p3e2-qps-ranker-20260928"
QPS_COPY = RANKER / "qps"
SCREEN = EVAL / "lt9-la2-p1p3e2-expanded-final-20260928"
RANKED = EVAL / "lt9-la2-p1p3e2-qps-ranks-run3-20260928"
RANKED_RERUN = EVAL / "lt9-la2-p1p3e2-qps-ranks-run2-20260928"
PACKETS = EVAL / "lt9-la2-p1p3e2-context-bank-final-20260928"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_record(path: Path, *, display: str | None = None) -> dict:
    return {
        "path": display or str(path),
        "sha256": sha256_file(path),
        "bytes": path.stat().st_size,
    }


def tree_records(root: Path) -> list[dict]:
    files = sorted(path for path in root.rglob("*") if path.is_file())
    return [file_record(path, display=path.relative_to(root).as_posix()) for path in files]


def tree_hash(records: list[dict]) -> str:
    digest = hashlib.sha256()
    for row in records:
        digest.update(row["path"].encode("utf-8"))
        digest.update(b"\0")
        digest.update(row["sha256"].encode("ascii"))
        digest.update(b"\n")
    return digest.hexdigest()


def git_text(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main() -> None:
    source_names = [
        "PROTOCOL.md",
        "EXPANDED_DISCOVERY_PROTOCOL.md",
        "CONTEXT_REVIEW_PROTOCOL.md",
        "BASELINE_BUILD.md",
        "P1P3E2_DISCOVERY_REPORT.md",
        "cohort.json",
        "candidate-relations.json",
        "candidate-relations-expanded.json",
        "src/main.rs",
        "qps_baseline.rs",
        "make_context_packets.py",
        "make_manifest.py",
        "context-rubric.md",
        "test_make_context_packets.py",
        "Cargo.toml",
        "Cargo.lock",
        ".gitignore",
    ]
    source_files = [file_record(EXPERIMENT / name, display=f"experiments/{EXPERIMENT.name}/{name}") for name in source_names]

    screen_receipt = json.loads((SCREEN / "opportunity-screen-receipt.json").read_text(encoding="utf-8"))
    qps_source_records = tree_records(QPS_COPY)
    rank_run2 = RANKED_RERUN / "ranked-opportunity-candidates.jsonl"
    rank_run3 = RANKED / "ranked-opportunity-candidates.jsonl"
    rank_sha2 = sha256_file(rank_run2)
    rank_sha3 = sha256_file(rank_run3)
    output_names = [
        "opportunity-screen-receipt.json",
        "qrels-counterpart-candidates.jsonl",
    ]
    output_files = [file_record(SCREEN / name) for name in output_names]
    output_files.extend(
        file_record(RANKED / name)
        for name in ("qps-baseline-receipt.json", "ranked-opportunity-candidates.jsonl")
    )
    output_files.extend(
        file_record(PACKETS / name)
        for name in (
            "acquisition-receipt.json",
            "review-packets.json",
            "judgments-template.json",
            "private-ledger.json",
            "transport-opportunity-bank.jsonl",
            "rubric.md",
        )
    )

    qps_root = json.loads((RANKED / "qps-baseline-receipt.json").read_text(encoding="utf-8"))
    binary_path = Path(r"D:\phoenix-target-overgraph\release\p1p3e2-qps-baseline.exe")
    harness_files = [RANKER / name for name in ("Cargo.toml", "Cargo.lock", "src/main.rs")]

    manifest = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2-artifact-manifest/v1",
        "date": "2026-09-28",
        "branch": git_text("branch", "--show-current"),
        "base_commit": git_text("rev-parse", "HEAD"),
        "status": "DISCOVERY_ONLY_OPPORTUNITIES_FOUND_CONTEXT_REVIEW_READY_NO_FIT",
        "source_files": source_files,
        "screen_runner": {
            "source_sha256": screen_receipt["runner_source_sha256"],
            "binary_sha256": screen_receipt["runner_binary_sha256"],
            "input_hashes": screen_receipt["input_hashes"],
        },
        "qps_baseline": {
            "binary": file_record(binary_path),
            "harness_files": [file_record(path) for path in harness_files],
            "qps_source_files": qps_source_records,
            "qps_source_tree_sha256": tree_hash(qps_source_records),
            "ranked_jsonl_run2_sha256": rank_sha2,
            "ranked_jsonl_run3_sha256": rank_sha3,
            "deterministic_rank_output_match": rank_sha2 == rank_sha3,
            "receipt_sha256": sha256_file(RANKED / "qps-baseline-receipt.json"),
            "candidate_rows": qps_root["candidate_rows"],
            "top100_misses": qps_root["missed_top100"],
            "rank_11_100": qps_root["underranked_11_100"],
            "top10": qps_root["already_top10"],
            "unsupported_candidate_rows": qps_root["unsupported_candidate_rows"],
        },
        "outputs": output_files,
        "preserved_attempts": [
            {
                "path": str(EVAL / "lt9-la2-p1p3e2-expanded-20260928"),
                "disposition": "FAILED_STACK_OVERFLOW_DURING_RECEIPT_HASH; superseded by expanded-final",
            },
            {
                "path": str(EVAL / "lt9-la2-p1p3e2-qps-ranks-final-20260928"),
                "disposition": "INCOMPLETE_QPS_RUN; no result interpreted; superseded by run3",
            },
            {
                "path": str(EVAL / "lt9-la2-p1p3e2-qps-ranks-retry-20260928"),
                "disposition": "INTERRUPTED_DURING_MEMORY-MONITORED_BUILD; no result interpreted",
            },
            {
                "path": str(EVAL / "lt9-la2-p1p3e2-context-bank-20260928"),
                "disposition": "FAILED_PACKET_QA; stored excerpts omitted target occurrences and unordered lexical_pair hid direction; superseded by context-bank-final",
            },
        ],
        "boundary": {
            "feature_join": False,
            "model_fit_or_threshold_selection": False,
            "authority_update": False,
            "serving_change": False,
            "qrels_absence_used_as_negative": False,
            "formal_p1p3_qualification": False,
        },
    }
    output_path = EXPERIMENT / "P1P3E2_ARTIFACT_MANIFEST.json"
    output_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {output_path}")
    print(f"rank output deterministic={rank_sha2 == rank_sha3} sha256={rank_sha3}")
    print(f"manifest sha256={sha256_file(output_path)} bytes={output_path.stat().st_size}")


if __name__ == "__main__":
    main()
