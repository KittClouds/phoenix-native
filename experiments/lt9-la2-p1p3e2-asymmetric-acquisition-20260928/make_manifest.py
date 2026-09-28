"""Bind P1P3E2-A1 source files and local discovery/review artifacts by SHA-256."""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(r"D:\phoenix-native-cleanroom-cut4-20260920")
EXPERIMENT = ROOT / "experiments/lt9-la2-p1p3e2-asymmetric-acquisition-20260928"
EVAL = Path(r"D:\phoenix-evals")
SCREEN = EVAL / "lt9-la2-p1p3e2-asymmetric-acq-screen-20260928"
RANKS = EVAL / "lt9-la2-p1p3e2-asymmetric-acq-ranks-20260928"
RANKS_RERUN = EVAL / "lt9-la2-p1p3e2-asymmetric-acq-ranks-rerun-20260928"
PACKETS = EVAL / "lt9-la2-p1p3e2-asym-fit-context-review-20260928"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def record(path: Path, display: str | None = None) -> dict:
    return {"path": display or str(path), "sha256": sha256(path), "bytes": path.stat().st_size}


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main() -> None:
    source_names = [
        ".gitignore",
        "PROTOCOL.md",
        "cohort.json",
        "candidate-relations.json",
        "context-rubric.md",
        "make_fit_review_packets.py",
        "make_manifest.py",
        "P1P3E2A1_DISCOVERY_REPORT.md",
    ]
    sources = [
        record(EXPERIMENT / name, f"experiments/{EXPERIMENT.name}/{name}")
        for name in source_names
    ]
    external_paths = [
        (SCREEN / "opportunity-screen-receipt.json", "screen"),
        (SCREEN / "qrels-counterpart-candidates.jsonl", "screen"),
        (RANKS / "qps-baseline-receipt.json", "rank-primary"),
        (RANKS / "ranked-opportunity-candidates.jsonl", "rank-primary"),
        (RANKS_RERUN / "qps-baseline-receipt.json", "rank-rerun"),
        (RANKS_RERUN / "ranked-opportunity-candidates.jsonl", "rank-rerun"),
        (PACKETS / "acquisition-receipt.json", "fit-only-context-review"),
        (PACKETS / "review-packets.json", "fit-only-context-review"),
        (PACKETS / "judgments-template.json", "fit-only-context-review"),
        (PACKETS / "private-ledger.json", "fit-only-context-review-private"),
        (PACKETS / "transport-opportunity-bank.jsonl", "fit-only-context-review-private"),
        (PACKETS / "rubric.md", "fit-only-context-review"),
    ]
    external = [
        {**record(path), "role": role}
        for path, role in external_paths
    ]
    screen_receipt = json.loads((SCREEN / "opportunity-screen-receipt.json").read_text(encoding="utf-8"))
    rank_receipt = json.loads((RANKS / "qps-baseline-receipt.json").read_text(encoding="utf-8"))
    primary_rank = RANKS / "ranked-opportunity-candidates.jsonl"
    rerun_rank = RANKS_RERUN / "ranked-opportunity-candidates.jsonl"
    packet_receipt = json.loads((PACKETS / "acquisition-receipt.json").read_text(encoding="utf-8"))
    manifest = {
        "schema": "phoenix.lexical.lt9-la2-p1p3e2a1-artifact-manifest/v1",
        "date": "2026-09-28",
        "branch": git("branch", "--show-current"),
        "base_commit": git("rev-parse", "HEAD"),
        "status": "DISCOVERY_ONLY_NO_MODEL_FIT_NO_HOLDOUT_REVIEWED",
        "source_files": sources,
        "screen_runner_source_sha256": screen_receipt["runner_source_sha256"],
        "screen_runner_binary_sha256": screen_receipt["runner_binary_sha256"],
        "qps_ranker_receipt_sha256": sha256(RANKS / "qps-baseline-receipt.json"),
        "qps_ranked_output_deterministic": sha256(primary_rank) == sha256(rerun_rank),
        "qps_ranked_output_sha256": sha256(primary_rank),
        "fit_packet_count": packet_receipt["packet_count"],
        "fit_opportunity_packet_count": packet_receipt["opportunity_count"],
        "sealed_holdout_candidate_rows": packet_receipt["holdout_candidate_rows_excluded"],
        "sealed_holdout_opportunity_rows": packet_receipt["holdout_opportunity_rows_excluded"],
        "external_artifacts": external,
    }
    path = EXPERIMENT / "P1P3E2A1_ARTIFACT_MANIFEST.json"
    path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {path}")
    print(f"manifest sha256={sha256(path)} bytes={path.stat().st_size}")


if __name__ == "__main__":
    main()
