#!/usr/bin/env python3
"""Build a directed cross-context transport bank from SWORDS and CoInCo."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import json
import random
import unicodedata
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA = "phoenix.lexical.synthetic-transport-bank/v1"
DATASETS = (
    ("swords-v1.1_dev.json.gz", "SWORDS_V1_1_DEV"),
    ("coinco_dev.json.gz", "COINCO_DEV"),
)
SPLIT_THRESHOLDS = ((70, "train"), (85, "dev"), (100, "test"))


@dataclass(frozen=True)
class Observation:
    observation_id: str
    dataset: str
    context_id: str
    context_occurrence_id: str
    context_group_id: str
    context_text: str
    masked_context: str
    source_surface: str
    source_lemma: str
    target_surface: str
    target_lemma: str
    pos: str
    label: str
    evidence_type: str
    confidence: float
    vote_counts: dict[str, int]
    split: str


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalized_text(text: str) -> str:
    return " ".join(unicodedata.normalize("NFKC", text).casefold().split())


def normalized_term(text: str) -> str:
    return " ".join(unicodedata.normalize("NFKC", text).casefold().split())


def stable_split(context_group_id: str) -> str:
    bucket = int(context_group_id[:8], 16) % 100
    for upper, name in SPLIT_THRESHOLDS:
        if bucket < upper:
            return name
    raise AssertionError("unreachable split")


def context_lemma(target: dict[str, Any]) -> str:
    extra = target.get("extra")
    if isinstance(extra, dict):
        return str(extra.get("coinco_lemma") or target["target"])
    if isinstance(extra, list) and extra and isinstance(extra[0], dict):
        coinco = extra[0].get("coinco", {})
        attrs = coinco.get("xml_attrs", {}) if isinstance(coinco, dict) else {}
        return str(attrs.get("lemma") or target["target"])
    return str(target["target"])


def substitute_lemma(substitute: dict[str, Any]) -> str:
    extra = substitute.get("extra")
    if isinstance(extra, list) and extra and isinstance(extra[0], dict):
        coinco = extra[0].get("coinco", {})
        attrs = coinco.get("xml_attrs", {}) if isinstance(coinco, dict) else {}
        return str(attrs.get("lemma") or substitute["substitute"])
    return str(substitute["substitute"])


def mask_target(text: str, offset: int, surface: str) -> str:
    end = offset + len(surface)
    if offset < 0 or end > len(text) or text[offset:end].casefold() != surface.casefold():
        raise ValueError(f"target span mismatch at offset {offset}: {surface!r}")
    return text[:offset] + "[FOCAL]" + text[end:]


def source_label(
    dataset: str, labels: list[str]
) -> tuple[str, str, float, dict[str, int]]:
    counts = Counter(labels)
    if dataset == "SWORDS_V1_1_DEV":
        total = sum(counts.values())
        true = counts["TRUE"]
        false = counts["FALSE"]
        unsure = counts["UNSURE"]
        if total and true * 3 >= total * 2:
            return "SAME", "SWORDS_TRUE_VOTE", true / total, dict(counts)
        if total and false * 3 >= total * 2:
            return "DIFFERENT", "SWORDS_FALSE_VOTE", false / total, dict(counts)
        uncertainty = 1.0 - max(true, false) / total if total else 0.0
        return "UNKNOWN", "SWORDS_UNSURE_OR_MIXED", uncertainty, dict(counts)

    positive = counts["TRUE_IMPLICIT"]
    negative = counts["FALSE_IMPLICIT"]
    if positive:
        return "SAME", "COINCO_TRUE_IMPLICIT", 0.65, dict(counts)
    if negative:
        return "DIFFERENT", "COINCO_FALSE_IMPLICIT_WEAK_NEGATIVE", 0.25, dict(counts)
    return "UNKNOWN", "COINCO_NO_LABEL", 0.0, dict(counts)


def read_dataset(path: Path, dataset: str) -> list[Observation]:
    with gzip.open(path, "rt", encoding="utf-8") as stream:
        data = json.load(stream)

    substitutes_by_target: dict[str, list[tuple[str, dict[str, Any]]]] = defaultdict(list)
    for substitute_id, substitute in data["substitutes"].items():
        substitutes_by_target[str(substitute["target_id"])].append((substitute_id, substitute))

    observations: list[Observation] = []
    contexts = data["contexts"]
    labels_by_id = data["substitute_labels"]
    for target_id, target in data["targets"].items():
        context = contexts[str(target["context_id"])]["context"]
        source_surface = str(target["target"])
        offset = int(target["offset"])
        masked = mask_target(context, offset, source_surface)
        group_id = sha256_bytes(normalized_text(context).encode("utf-8"))
        source_lemma = normalized_term(context_lemma(target))
        occurrence_seed = "\0".join((group_id, str(offset), source_surface.casefold(), source_lemma))
        occurrence_id = sha256_bytes(occurrence_seed.encode("utf-8"))
        pos = str(target.get("pos") or "UNKNOWN")
        for substitute_id, substitute in substitutes_by_target[str(target_id)]:
            surface = str(substitute["substitute"]).strip()
            lemma = normalized_term(substitute_lemma(substitute))
            if not surface or not lemma or lemma == source_lemma:
                continue
            label, evidence_type, confidence, votes = source_label(
                dataset, list(labels_by_id[str(substitute_id)])
            )
            obs_seed = "\0".join((dataset, group_id, str(offset), source_lemma, lemma))
            observations.append(
                Observation(
                    observation_id=sha256_bytes(obs_seed.encode("utf-8")),
                    dataset=dataset,
                    context_id=str(target["context_id"]),
                    context_occurrence_id=occurrence_id,
                    context_group_id=group_id,
                    context_text=context,
                    masked_context=masked,
                    source_surface=source_surface,
                    source_lemma=source_lemma,
                    target_surface=surface,
                    target_lemma=lemma,
                    pos=pos,
                    label=label,
                    evidence_type=evidence_type,
                    confidence=confidence,
                    vote_counts=votes,
                    split=stable_split(group_id),
                )
            )
    return observations


def deduplicate_observations(rows: Iterable[Observation]) -> list[Observation]:
    priority = {"SWORDS_V1_1_DEV": 0, "COINCO_DEV": 1}
    selected: dict[tuple[str, str, str], Observation] = {}
    for row in rows:
        key = (row.context_occurrence_id, row.source_lemma, row.target_lemma)
        old = selected.get(key)
        if old is None or priority[row.dataset] < priority[old.dataset]:
            selected[key] = row
    return sorted(
        selected.values(),
        key=lambda row: (row.source_lemma, row.target_lemma, row.context_group_id, row.dataset),
    )


def sample_cross_pairs(
    left_rows: list[Observation],
    right_rows: list[Observation],
    relation_key: str,
    split: str,
    label: str,
    cap: int,
) -> list[tuple[Observation, Observation]]:
    if not left_rows or not right_rows:
        return []
    seed = int(sha256_bytes(f"{relation_key}|{split}|{label}".encode())[:16], 16)
    rng = random.Random(seed)
    max_attempts = max(cap * 16, 64)
    selected: list[tuple[Observation, Observation]] = []
    seen: set[tuple[str, str]] = set()
    left_n, right_n = len(left_rows), len(right_rows)
    for _ in range(max_attempts):
        left = left_rows[rng.randrange(left_n)]
        right = right_rows[rng.randrange(right_n)]
        if left.context_group_id == right.context_group_id:
            continue
        key = (left.observation_id, right.observation_id)
        if key in seen:
            continue
        seen.add(key)
        selected.append((left, right))
        if len(selected) >= cap:
            break
    return selected


def pair_label(left: Observation, right: Observation) -> tuple[str, float, str]:
    if left.label == "DIFFERENT" or right.label == "DIFFERENT":
        labels = "REJECT" if left.label == "DIFFERENT" and right.label == "DIFFERENT" else "ONE_ENDPOINT_REJECTS"
        return "DIFFERENT", min(left.confidence, right.confidence), labels
    if left.label == "SAME" and right.label == "SAME":
        return "SAME", min(left.confidence, right.confidence), "BOTH_ENDPOINTS_ACCEPT"
    return "UNKNOWN", min(left.confidence, right.confidence), "SEED_UNSURE_OR_MIXED"


def build_pairs(observations: list[Observation], per_cell_cap: int) -> list[dict[str, Any]]:
    directed: dict[tuple[str, str], list[Observation]] = defaultdict(list)
    for row in observations:
        directed[(row.source_lemma, row.target_lemma)].append(row)

    generated: list[dict[str, Any]] = []
    relation_keys = sorted(directed)
    for source, target in relation_keys:
        left_all = directed[(source, target)]
        right_all = directed.get((target, source), [])
        if not right_all:
            continue
        for split in ("train", "dev", "test"):
            left_by = {
                label: [row for row in left_all if row.split == split and row.label == label]
                for label in ("SAME", "DIFFERENT", "UNKNOWN")
            }
            right_by = {
                label: [row for row in right_all if row.split == split and row.label == label]
                for label in ("SAME", "DIFFERENT", "UNKNOWN")
            }
            relation_key = f"{source}->{target}"
            selected: list[tuple[Observation, Observation]] = []
            selected.extend(sample_cross_pairs(left_by["SAME"], right_by["SAME"], relation_key, split, "SAME", per_cell_cap))
            # Any explicit rejection at either endpoint makes transport unsafe.
            for left_label, right_label in (
                ("SAME", "DIFFERENT"),
                ("DIFFERENT", "SAME"),
                ("DIFFERENT", "DIFFERENT"),
                ("UNKNOWN", "DIFFERENT"),
                ("DIFFERENT", "UNKNOWN"),
            ):
                selected.extend(sample_cross_pairs(left_by[left_label], right_by[right_label], relation_key, split, f"{left_label}:{right_label}", per_cell_cap))
            for left_label, right_label in (("UNKNOWN", "SAME"), ("SAME", "UNKNOWN"), ("UNKNOWN", "UNKNOWN")):
                selected.extend(sample_cross_pairs(left_by[left_label], right_by[right_label], relation_key, split, f"{left_label}:{right_label}", per_cell_cap))

            seen_pairs: set[tuple[str, str]] = set()
            for left, right in selected:
                identity = (left.context_occurrence_id, right.context_occurrence_id)
                if identity in seen_pairs:
                    continue
                seen_pairs.add(identity)
                label, confidence, rule = pair_label(left, right)
                if label == "UNKNOWN" and rule == "SEED_UNSURE_OR_MIXED":
                    origin = "SEED_UNSURE_OR_MIXED"
                else:
                    origin = "CROSS_CONTEXT_SUBSTITUTION_JOIN"
                base = "\0".join((relation_key, split, left.context_group_id, right.context_group_id, label))
                base_id = sha256_bytes(base.encode("utf-8"))
                generated.append(
                    {
                        "pair_id": base_id,
                        "base_group_id": base_id,
                        "split": split,
                        "direction": f"{source}->{target}",
                        "source_lemma": source,
                        "target_lemma": target,
                        "left_context_id": left.context_occurrence_id,
                        "right_context_id": right.context_occurrence_id,
                        "left_observation_id": left.observation_id,
                        "right_observation_id": right.observation_id,
                        "left_context_group_id": left.context_group_id,
                        "right_context_group_id": right.context_group_id,
                        "label": label,
                        "label_strength": round(confidence, 4),
                        "label_origin": origin,
                        "construction": rule,
                        "left_evidence": left.evidence_type,
                        "right_evidence": right.evidence_type,
                        "left_dataset": left.dataset,
                        "right_dataset": right.dataset,
                    }
                )

    # One paired no-evidence variant per definite joined example.
    synthetic: list[dict[str, Any]] = []
    for row in generated:
        row["context_transform"] = "NONE"
        if row["label"] == "UNKNOWN":
            continue
        variant = dict(row)
        variant["pair_id"] = sha256_bytes((row["pair_id"] + "|erase-both").encode())
        variant["base_group_id"] = row["base_group_id"]
        variant["context_transform"] = "BOTH_ENDPOINTS_ERASED"
        variant["label"] = "UNKNOWN"
        variant["label_strength"] = 1.0
        variant["label_origin"] = "SYNTHETIC_NO_LOCAL_EVIDENCE"
        variant["construction"] = "BOTH_ENDPOINT_CONTEXTS_ERASED"
        synthetic.append(variant)
    return sorted(generated + synthetic, key=lambda row: (row["split"], row["direction"], row["pair_id"]))


def write_jsonl_gzip(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    count = 0
    with path.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=6, mtime=0) as compressed:
            with io.TextIOWrapper(compressed, encoding="utf-8", newline="\n") as stream:
                for row in rows:
                    stream.write(json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
                    stream.write("\n")
                    count += 1
    return count


def validate_bank(
    observations: list[Observation],
    contexts: dict[str, dict[str, Any]],
    pairs: list[dict[str, Any]],
) -> dict[str, Any]:
    by_observation = {row.observation_id: row for row in observations}
    occurrence_ids = {row.context_occurrence_id for row in observations}
    context_splits = {key: row["split"] for key, row in contexts.items()}
    seen_groups: dict[str, str] = {}
    for context_id, split in context_splits.items():
        context_group = contexts[context_id]["context_group_id"]
        prior = seen_groups.setdefault(context_group, split)
        if prior != split:
            raise ValueError("one normalized natural context crossed split boundaries")

    generated_counts: Counter[str] = Counter()
    source_evidence: Counter[str] = Counter()
    for pair in pairs:
        left_id = pair["left_context_id"]
        right_id = pair["right_context_id"]
        if left_id not in contexts or right_id not in contexts:
            raise ValueError("pair references a missing context occurrence")
        if left_id not in occurrence_ids:
            raise ValueError("left context id is not an observed occurrence")
        if right_id not in occurrence_ids:
            raise ValueError("right context id is not an observed occurrence")
        if context_splits[left_id] != pair["split"] or context_splits[right_id] != pair["split"]:
            raise ValueError("pair crossed a context partition")
        left = by_observation.get(pair["left_observation_id"])
        right = by_observation.get(pair["right_observation_id"])
        if left is None or right is None:
            raise ValueError("pair references a missing substitution observation")
        if left.context_occurrence_id != left_id or right.context_occurrence_id != right_id:
            raise ValueError("pair context and substitution observation disagree")
        if left.source_lemma != pair["source_lemma"] or right.source_lemma != pair["target_lemma"]:
            raise ValueError("pair endpoint lemmas do not match directed relation")
        if pair["context_transform"] == "BOTH_ENDPOINTS_ERASED":
            if pair["label"] != "UNKNOWN" or pair["label_origin"] != "SYNTHETIC_NO_LOCAL_EVIDENCE":
                raise ValueError("erased context row must be a synthetic UNKNOWN")
            source_evidence[pair["label_origin"]] += 1
        else:
            expected, _, _ = pair_label(left, right)
            if pair["label"] != expected:
                raise ValueError("joined label does not match endpoint seed evidence")
            source_evidence[pair["label_origin"]] += 1
        generated_counts[pair["label"]] += 1

    return {
        "status": "BANK_VALIDATED",
        "pairs_checked": len(pairs),
        "context_occurrences_checked": len(contexts),
        "observation_rows_checked": len(observations),
        "label_counts": dict(generated_counts),
        "label_origins": dict(source_evidence),
        "context_groups_crossing_splits": 0,
    }


def build(swords_root: Path, output: Path, script_path: Path) -> dict[str, Any]:
    if output.exists():
        raise FileExistsError(f"refusing to overwrite output directory: {output}")
    assets = swords_root / "assets" / "parsed"
    source_paths = {name: assets / name for name, _ in DATASETS}
    missing = [str(path) for path in source_paths.values() if not path.is_file()]
    if missing:
        raise FileNotFoundError("missing seed archive(s): " + ", ".join(missing))

    output.mkdir(parents=True)
    raw: list[Observation] = []
    source_counts: dict[str, int] = {}
    for filename, dataset in DATASETS:
        rows = read_dataset(source_paths[filename], dataset)
        source_counts[dataset] = len(rows)
        raw.extend(rows)
    observations = deduplicate_observations(raw)

    contexts: dict[str, dict[str, Any]] = {}
    for row in observations:
        contexts.setdefault(
            row.context_occurrence_id,
            {
                "context_occurrence_id": row.context_occurrence_id,
                "source_context_id": row.context_id,
                "context_group_id": row.context_group_id,
                "split": row.split,
                "dataset": row.dataset,
                "source_lemma": row.source_lemma,
                "pos": row.pos,
                "context": row.context_text,
                "masked_context": row.masked_context,
            },
        )
    observation_rows = []
    for row in observations:
        record = asdict(row)
        record.pop("context_text")
        record.pop("masked_context")
        observation_rows.append(record)
    pairs = build_pairs(observations, per_cell_cap=6)
    validation = validate_bank(observations, contexts, pairs)

    contexts_path = output / "contexts.jsonl.gz"
    observations_path = output / "substitution-observations.jsonl.gz"
    pairs_path = output / "transport-pairs.jsonl.gz"
    context_count = write_jsonl_gzip(contexts_path, (contexts[key] for key in sorted(contexts)))
    observation_count = write_jsonl_gzip(observations_path, observation_rows)
    pair_count = write_jsonl_gzip(pairs_path, pairs)

    label_counts: dict[str, Counter[str]] = defaultdict(Counter)
    direction_counts: dict[str, Counter[str]] = defaultdict(Counter)
    split_counts: dict[str, Counter[str]] = defaultdict(Counter)
    for row in pairs:
        label_counts[row["split"]][row["label"]] += 1
        direction_counts[row["direction"]][row["label"]] += 1
        split_counts[row["split"]][row["construction"]] += 1

    repo_revision = "unknown"
    head_path = swords_root / ".git" / "HEAD"
    if head_path.exists():
        try:
            import subprocess

            repo_revision = subprocess.check_output(
                ["git", "-C", str(swords_root), "rev-parse", "HEAD"], text=True
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            pass

    receipt = {
        "schema": SCHEMA,
        "status": "SYNTHETIC_ENGINEERING_BANK_BUILT",
        "date": "2026-09-28",
        "upstream": {
            "repository": "https://github.com/p-lambda/swords",
            "revision": repo_revision,
            "readme_sha256": sha256_file(swords_root / "README.md"),
            "license_note": "SWORDS/CoInCo/MASC CC-BY-3.0-US per upstream README",
        },
        "inputs": {
            filename: {"sha256": sha256_file(path), "bytes": path.stat().st_size}
            for filename, path in source_paths.items()
        },
        "generator_sha256": sha256_file(script_path),
        "raw_observations_by_source": source_counts,
        "deduplicated_observations": observation_count,
        "unique_context_occurrences": context_count,
        "transport_pair_rows": pair_count,
        "validation": validation,
        "label_counts_by_split": {key: dict(value) for key, value in sorted(label_counts.items())},
        "construction_counts_by_split": {key: dict(value) for key, value in sorted(split_counts.items())},
        "directed_relations": len(direction_counts),
        "max_pairs_per_relation_class_split": 6,
        "split_rule": "sha256(normalized full context) modulo 100: 0-69 train, 70-84 dev, 85-99 test",
        "label_semantics": {
            "SAME": "both directed substitution endpoints have positive seed evidence",
            "DIFFERENT": "one or both directed substitution endpoints have rejection evidence",
            "UNKNOWN": "seed uncertainty/mixed evidence or generated both-endpoint erasure",
            "COINCO_FALSE_IMPLICIT": "weak negative: candidate was not elicited, not an explicit rejection",
        },
        "synthetic_unknown_rows": sum(1 for row in pairs if row["label_origin"] == "SYNTHETIC_NO_LOCAL_EVIDENCE"),
        "outputs": {
            path.name: {"sha256": sha256_file(path), "bytes": path.stat().st_size}
            for path in (contexts_path, observations_path, pairs_path)
        },
        "engineering_only": True,
        "formal_p1p3_qualification": False,
        "authority_updated": False,
        "retrieval_run": False,
    }
    (output / "build-receipt.json").write_text(
        json.dumps(receipt, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--swords-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    receipt = build(args.swords_root, args.output, Path(__file__).resolve())
    print(json.dumps({key: receipt[key] for key in (
        "status", "raw_observations_by_source", "deduplicated_observations",
        "unique_context_occurrences", "transport_pair_rows", "label_counts_by_split",
        "synthetic_unknown_rows", "directed_relations",
    )}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
